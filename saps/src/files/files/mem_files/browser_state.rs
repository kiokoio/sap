//! The in-memory file store for the browser wasm module.
//!
//! Every file the session has open is held as a [`MemFile`] in the thread-local
//! [`FILE_STATE`] map, keyed by path. That buffer is the live, editable CRDT the editor and
//! LSP read. Unlike the server session there is no relay-vs-cache split here: each buffer is
//! backed by the browser's IndexedDB through the [`IndexedDbIo`] async engine, so the buffer's
//! own `save` is what persists edits — and it persists them to the exact store the compiler
//! reads its source from, which is how the editor's live edits reach the compile.
//!
//! ## Why the state is thread-local
//!
//! The wasm runtime is single-threaded, so there is nothing to share across threads; and under
//! `wasm_bindgen_test` each test runs in the one browser context, so tests share the map and
//! must wipe it between runs (see the tests). Reach the map only through the free functions
//! below — each one takes the borrow, does its work, and releases it. Never hold a
//! `FILE_STATE` borrow across an `.await`, or across another `FILE_STATE` access, or the inner
//! `RefCell` panics on the re-entrant borrow.
//!
//! A save only needs the store, the path and the contents, never the buffer itself, so
//! [`apply_update_to_file`] and [`flush_all`] edit the buffer in place under a short borrow,
//! copy those three out (the store is `&'static`), release the borrow and then await the write.
//! [`write_transaction_to_file`] does the same, applying every operation in place before its
//! one save. The buffer stays in the map throughout, so a [`read_file`] or another edit while a
//! save is in flight finds it as normal. Taking the buffer *out* of the map for the await
//! instead would hide the file for the whole write: a merge into it would fail as "not open"
//! and lose its update, and a [`remove_file`] during the write would be undone when the buffer
//! was put back.
//!
//! Overlapping saves of one file land in order: `IndexedDbIo::write_file` issues its `put`
//! before its first await, and IndexedDB runs readwrite transactions on the same store in the
//! order they were created, so an older write can never land after a newer one.
//!
//! ## The store is `'static`
//!
//! A [`MemFile<Async<..>>`](MemFile) borrows its store, but a `thread_local` map must hold a
//! `'static` type, so the store cannot simply be borrowed from another `thread_local`. Instead
//! [`configure_persistence`] opens the IndexedDB database once and leaks it, handing every
//! buffer a `&'static IndexedDbIo`. The store lives for the whole session, so the leak is
//! deliberate and bounded — one store per configured database. Because a buffer needs the
//! store to exist before it can be built, persistence must be configured before any file is
//! opened.

use std::cell::RefCell;
use std::collections::HashMap;

use crate::errors::file::FileError;
use crate::kernel::transaction::{
    CursorIndex, DeleteSlice, InsertSlice, Operation, SwapSlice, Transaction,
};

use crate::files::engines::async_io::indexed_db::IndexedDbIo;
use crate::files::files::mem_files::constants::SAVE_THRESHOLD;
use crate::files::files::mem_files::full_descriptors::tx_utils;
use crate::files::files::mem_files::full_mem_file::{Async, MemFile};
use crate::files::io::async_file::AsyncFileIo;
use crate::files::paths::{FilePath, Path};

/// A browser buffer: a `MemFile` whose async saves persist to IndexedDB, holding a `'static`
/// borrow of the leaked store so it can live in the thread-local map.
type BrowserFile = MemFile<Async<'static, IndexedDbIo>>;

thread_local! {
    /// The leaked IndexedDB store every buffer flushes through, or `None` until
    /// [`configure_persistence`] has run. Held as a `&'static` because the buffers borrow it
    /// and must be `'static` to live in [`FILE_STATE`].
    static STORE: RefCell<Option<&'static IndexedDbIo>> = const { RefCell::new(None) };

    /// The files this session has open, keyed by path.
    ///
    /// Thread-local so the wasm runtime's single thread owns one map. Access it through the
    /// free functions below; never hold a borrow across an `.await` or another access.
    static FILE_STATE: RefCell<HashMap<String, BrowserFile>> = RefCell::new(HashMap::new());
}

/// Points every buffer's persistence at the IndexedDB database `db_name`, opening it and
/// turning persistence on. Must run before any file is opened.
///
/// The database is opened once and leaked so its handle is `'static` — every buffer built
/// afterwards borrows it (see the module docs). A later call re-targets a new database (for
/// example on a project switch); the previous store stays leaked, which is bounded to one per
/// configured database and lives for the session anyway.
///
/// # Arguments
/// - `db_name`: The IndexedDB database to open and persist buffers to. This is the same store
///   the compiler reads its source from.
///
/// # Returns
/// `Ok(())` once the database is open and set as the store, or a `FileError` if it could not
/// be opened.
pub async fn configure_persistence(db_name: &str) -> Result<(), FileError> {
    let store = IndexedDbIo::new(db_name).await?;
    let leaked: &'static IndexedDbIo = Box::leak(Box::new(store));
    STORE.with(|current| *current.borrow_mut() = Some(leaked));
    Ok(())
}

/// Hands back the configured `'static` store, or an error if persistence has not been set up.
fn store() -> Result<&'static IndexedDbIo, FileError> {
    STORE.with(|current| {
        current.borrow().ok_or_else(|| FileError::MemFile {
            path: String::new(),
            message: "persistence not configured; call configure_persistence first".into(),
        })
    })
}

/// Builds a `FileError::MemFile` for an operation aimed at a path that is not open.
fn not_open(path: &str, action: &str) -> FileError {
    FileError::MemFile {
        path: path.to_string(),
        message: format!("file not open to {action}"),
    }
}

/// Opens a file into the session by joining an existing collaborative session from the origin's
/// `yrs` state, then persists the seeded text.
///
/// This is the join path: `state` is the authoritative origin's state update (as the server's
/// `encode_state` produces), so the buffer adopts the origin's CRDT identities and can keep
/// merging edits. The seeded text is saved straight away so the compiler — which reads the same
/// IndexedDB store — sees the file. Any buffer already open at `path` is replaced.
///
/// # Arguments
/// - `path`: The key to store the buffer under, and the file it persists to.
/// - `state`: The origin's `yrs` state update to seed the buffer from.
///
/// # Returns
/// `Ok(())` once the buffer is seeded, saved, and stored, or a `FileError` if persistence is
/// off, the path is not a valid file path, the state cannot be decoded, or the save fails.
pub async fn insert_file_from_state(path: &str, state: &[u8]) -> Result<(), FileError> {
    let store = store()?;
    let file_path = Path::<FilePath>::new(path)?;
    let mut file = MemFile::from_state(file_path, state, Async::new(store))?;
    file.save().await?;
    FILE_STATE.with_borrow_mut(|state| {
        state.insert(path.to_string(), file);
    });
    Ok(())
}

/// Opens a file into the session seeded directly from `contents`, then persists it.
///
/// This is the local path — a newly created file, or one seeded from text the caller already
/// holds, rather than joined from an origin's CRDT state. It mints fresh CRDT identities, so a
/// file that will later be collaborated on should instead join with
/// [`insert_file_from_state`]. The seeded text is saved straight away so the compiler sees it.
/// Any buffer already open at `path` is replaced.
///
/// # Arguments
/// - `path`: The key to store the buffer under, and the file it persists to.
/// - `contents`: The starting text of the buffer.
///
/// # Returns
/// `Ok(())` once the buffer is seeded, saved, and stored, or a `FileError` if persistence is
/// off, the path is not a valid file path, or the save fails.
pub async fn insert_file_from_string(path: &str, contents: String) -> Result<(), FileError> {
    let store = store()?;
    let file_path = Path::<FilePath>::new(path)?;
    let mut file = MemFile::from_string(file_path, Async::new(store), contents);
    file.save().await?;
    FILE_STATE.with_borrow_mut(|state| {
        state.insert(path.to_string(), file);
    });
    Ok(())
}

/// Removes the buffer at `path` from the session. A path that is not open is a no-op.
///
/// The buffer is dropped, not flushed: edits are persisted as they happen (see the module
/// docs), so there is nothing to save on the way out that active editing has not already
/// written. Used to close a file, and by a delete or move that has relocated it.
///
/// # Arguments
/// - `path`: The key to drop from the session.
pub fn remove_file(path: &str) {
    FILE_STATE.with_borrow_mut(|state| {
        state.remove(path);
    });
}

/// Drops every open buffer, leaving the session empty.
///
/// Used to reset the session — between tests, and when the whole session is torn down (for
/// example on sign-out). The configured store is left in place.
pub fn wipe_state() {
    let _ = FILE_STATE.take();
}

/// Applies a transaction to the buffer at `path`.
///
/// Each operation is resolved against the buffer as it stands when that operation runs, so an
/// operation's position must account for the edits before it in the same transaction.
///
/// Every operation is applied in place, without IO, while the buffer stays in the map (see the
/// module docs). The transaction is then saved at most once, after the last operation, if the
/// saving policy calls for it: a bulk edit (a run longer than one character) always does, and
/// single character edits do once [`SAVE_THRESHOLD`] of them have built up since the last save.
///
/// # Arguments
/// - `path`: The file to apply the transaction to.
/// - `transaction`: The ordered edits to apply.
///
/// # Returns
/// `Ok(())` once the transaction is applied (and persisted, if a save was due), or a
/// `FileError` if no file is open at `path`, an edit fails, or the save fails. The operations
/// before a failing one stay applied, and reach IndexedDB with the next save.
pub async fn write_transaction_to_file(
    path: &str,
    transaction: &Transaction,
) -> Result<(), FileError> {
    let save = FILE_STATE.with_borrow_mut(|state| -> Result<_, FileError> {
        let file = state
            .get_mut(path)
            .ok_or_else(|| not_open(path, "apply a transaction to"))?;
        if !apply_transaction(file, transaction)? {
            return Ok(None);
        }
        // Reset now, not after the write, for the same reason as in `apply_update_to_file`.
        file.ops_since_save = 0;
        Ok(Some((
            file.backend.store,
            file.path.clone(),
            file.contents(),
        )))
    })?;
    match save {
        Some((store, file_path, contents)) => store.write_file(&file_path, contents).await,
        None => Ok(()),
    }
}

/// Merges a `yrs` update from a peer into the open buffer at `path`.
///
/// This is the inbound half of collaboration: the server broadcasts the `yrs` update its
/// authoritative buffer produced, and each replica integrates it here so its buffer converges.
/// Applying the update is idempotent and commutative — a replay or an out-of-order arrival
/// still converges. The merged text is then saved so the compiler sees it.
///
/// The merge happens in place and the buffer never leaves the map (see the module docs), so
/// updates that arrive back to back — a burst of echoes the server held behind a compile —
/// all merge, even while an earlier one's save is still in flight.
///
/// # Arguments
/// - `path`: The file whose buffer the update is merged into.
/// - `update`: A `yrs` v1 update, as produced by the server's `encode_diff`.
///
/// # Returns
/// `Ok(())` once merged and persisted, or a `FileError` if no file is open at `path`, the bytes
/// are not a valid `yrs` update, or the save fails.
pub async fn apply_update_to_file(path: &str, update: &[u8]) -> Result<(), FileError> {
    let (store, file_path, contents) =
        FILE_STATE.with_borrow_mut(|state| -> Result<_, FileError> {
            let file = state
                .get_mut(path)
                .ok_or_else(|| not_open(path, "apply an update to"))?;
            file.apply_update(update)?;
            // Reset now, not after the write: the counter is edits since the contents last handed
            // to the store, and edits made while this write is in flight are not in `contents`.
            file.ops_since_save = 0;
            Ok((file.backend.store, file.path.clone(), file.contents()))
        })?;
    store.write_file(&file_path, contents).await
}

/// Reads the current contents of the buffer at `path`.
///
/// Returns the buffer's live text — including edits applied but not yet seen by the server — so
/// it is what the editor and LSP render.
///
/// # Arguments
/// - `path`: The file to read.
///
/// # Returns
/// `Some(contents)` if a buffer is open at `path`, otherwise `None`.
pub fn read_file(path: &str) -> Option<String> {
    FILE_STATE.with_borrow(|state| state.get(path).map(|file| file.contents()))
}

/// Returns the paths of every open file that lies under `path`, sorted.
///
/// The state stores only files — there are no folder entries to walk — so a folder is just a
/// prefix shared by its files' paths. Matching is **component-wise** (via
/// [`Path::starts_with`](std::path::Path::starts_with)), so `path` `"src"` returns
/// `"src/main.cad"` and `"src/util/lib.cad"` but never the sibling `"src2/main.cad"`. A file
/// open at exactly `path` is included, and `path` `""` matches every open file (the root).
///
/// This is the primitive a folder-level operation builds on: a move, rename, or delete of a
/// folder gathers its files with this, then acts on each. The paths come back sorted so the
/// order is deterministic.
///
/// # Arguments
/// - `path`: The folder (or file) path to collect the descendants of.
pub fn subpaths_of(path: &str) -> Vec<String> {
    let prefix = std::path::Path::new(path);
    let mut matches: Vec<String> = FILE_STATE.with_borrow(|state| {
        state
            .keys()
            .filter(|key| std::path::Path::new(key.as_str()).starts_with(prefix))
            .cloned()
            .collect()
    });
    matches.sort();
    matches
}

/// Forces every open buffer's batched edits out to IndexedDB and awaits them, so a subsequent
/// read of the store (the compile) sees the latest edits.
///
/// Single character edits batch in the buffer and only flush once enough build up, so at any
/// moment some buffers can hold edits IndexedDB has not seen. This saves every one. Every
/// buffer's contents are copied out under one short borrow and the writes awaited afterwards,
/// so the buffers stay in the map (see the module docs): edits and merges arriving during the
/// flush land as normal, and are picked up by the next save.
///
/// # Returns
/// `Ok(())` once every buffer has been saved, or the last save error if any failed (every
/// buffer is still attempted).
pub async fn flush_all() -> Result<(), FileError> {
    let saves: Vec<_> = FILE_STATE.with_borrow_mut(|state| {
        state
            .values_mut()
            .map(|file| {
                file.ops_since_save = 0;
                (file.backend.store, file.path.clone(), file.contents())
            })
            .collect()
    });
    let mut result = Ok(());
    for (store, file_path, contents) in saves {
        if let Err(error) = store.write_file(&file_path, contents).await {
            result = Err(error);
        }
    }
    result
}

// MARK: - Transaction application

/// Applies every operation in `transaction` to `file` in list order, in place and without IO.
///
/// A swap is applied as a delete of its run followed by an insert of the replacement at the
/// same position, so the replacement may be a different length from the run it replaces.
///
/// # Returns
/// Whether the transaction is due a save under the buffer's saving policy: `true` if any
/// operation was a bulk edit, or if the single character edits since the last save have
/// reached [`SAVE_THRESHOLD`].
fn apply_transaction(file: &mut BrowserFile, transaction: &Transaction) -> Result<bool, FileError> {
    let mut bulk_edit = false;
    for operation in transaction.operations() {
        match operation {
            Operation::Insert(InsertSlice { position, content }) => {
                bulk_edit |= apply_insert(file, *position, content)?;
            }
            Operation::Delete(DeleteSlice { position, length }) => {
                bulk_edit |= apply_delete(file, *position, *length)?;
            }
            Operation::Swap(SwapSlice {
                position,
                length,
                content,
            }) => {
                bulk_edit |= apply_delete(file, *position, *length)?;
                bulk_edit |= apply_insert(file, *position, content)?;
            }
        }
    }
    Ok(bulk_edit || file.ops_since_save >= SAVE_THRESHOLD)
}

/// Inserts `content` at `position`. A single character is counted towards
/// [`SAVE_THRESHOLD`]; a longer run is a bulk edit; an empty run is nothing to do.
///
/// # Returns
/// Whether the insert was a bulk edit, which is always due a save.
fn apply_insert(
    file: &mut BrowserFile,
    position: CursorIndex,
    content: &str,
) -> Result<bool, FileError> {
    let mut characters = content.chars();
    match (characters.next(), characters.next()) {
        (None, _) => Ok(false),
        (Some(character), None) => {
            let contents = file.contents();
            tx_utils::insert_char(
                &contents,
                &mut file.text,
                &mut file.doc,
                &position,
                character,
            )?;
            file.ops_since_save += 1;
            Ok(false)
        }
        (Some(_), Some(_)) => {
            let contents = file.contents();
            tx_utils::insert_text(&contents, &mut file.text, &mut file.doc, &position, content)?;
            Ok(true)
        }
    }
}

/// Deletes `length` characters from `position`. One character is counted towards
/// [`SAVE_THRESHOLD`]; a longer run is a bulk edit; a zero length run is nothing to do.
///
/// # Returns
/// Whether the delete was a bulk edit, which is always due a save.
fn apply_delete(
    file: &mut BrowserFile,
    position: CursorIndex,
    length: usize,
) -> Result<bool, FileError> {
    match length {
        0 => Ok(false),
        1 => {
            let contents = file.contents();
            tx_utils::delete_char(&contents, &mut file.text, &mut file.doc, &position)?;
            file.ops_since_save += 1;
            Ok(false)
        }
        _ => {
            let contents = file.contents();
            tx_utils::delete_range(&contents, &mut file.text, &mut file.doc, &position, length)?;
            Ok(true)
        }
    }
}

// MARK: - Tests

#[cfg(test)]
mod tests {
    // IndexedDB only exists in a browser, so these run under `wasm_bindgen_test` against a
    // headless browser. The thread-local state is shared across tests in the one wasm context,
    // so every test wipes the map and configures its own distinct database first. Persistence
    // is checked by reading the IndexedDB store back through a fresh `IndexedDbIo`, proving the
    // compiler would see the same bytes.

    use super::*;
    use futures::join;
    use wasm_bindgen_test::*;

    wasm_bindgen_test_configure!(run_in_browser);

    fn file(name: &str) -> Path<FilePath> {
        Path::<FilePath>::new(name).unwrap()
    }

    /// Wipes the session and points persistence at `db_name`, so each test starts from a clean
    /// map and an isolated database.
    async fn setup(db_name: &str) {
        wipe_state();
        configure_persistence(db_name)
            .await
            .expect("configure persistence");
    }

    /// Reads `path` straight out of the IndexedDB database `db_name` through a fresh engine —
    /// what the compiler would see.
    async fn read_store(db_name: &str, path: &str) -> Option<String> {
        let io = IndexedDbIo::new(db_name).await.expect("open store");
        io.read_file(&file(path)).await.ok()
    }

    /// A single-insert transaction at a line and column.
    fn insert_at(line: usize, col: usize, text: &str) -> Transaction {
        Transaction::new(vec![Operation::Insert(InsertSlice {
            position: CursorIndex { line, col },
            content: text.to_string(),
        })])
    }

    #[wasm_bindgen_test]
    async fn insert_from_string_stores_and_persists() {
        setup("browser_state_insert_string").await;

        insert_file_from_string("a.cad", "hello".into())
            .await
            .expect("insert");

        // The buffer reads back live, and the seeded text reached IndexedDB for the compiler.
        assert_eq!(read_file("a.cad").as_deref(), Some("hello"));
        assert_eq!(
            read_store("browser_state_insert_string", "a.cad")
                .await
                .as_deref(),
            Some("hello")
        );
    }

    #[wasm_bindgen_test]
    async fn insert_from_state_joins_origin_contents() {
        setup("browser_state_insert_state").await;

        // Build an origin buffer's state the way the server would ship it.
        let origin_store = IndexedDbIo::new("browser_state_origin").await.unwrap();
        let origin =
            MemFile::from_string(file("a.cad"), Async::new(&origin_store), "shared".into());
        let state = origin.encode_state();

        insert_file_from_state("a.cad", &state)
            .await
            .expect("insert");

        assert_eq!(read_file("a.cad").as_deref(), Some("shared"));
    }

    #[wasm_bindgen_test]
    async fn remove_file_drops_it_from_state() {
        setup("browser_state_remove").await;
        insert_file_from_string("a.cad", "hello".into())
            .await
            .expect("insert");

        remove_file("a.cad");

        assert!(read_file("a.cad").is_none());
    }

    #[wasm_bindgen_test]
    async fn remove_unknown_file_is_a_noop() {
        setup("browser_state_remove_unknown").await;

        remove_file("ghost.cad");

        assert_eq!(FILE_STATE.with_borrow(|state| state.len()), 0);
    }

    #[wasm_bindgen_test]
    async fn wipe_state_clears_every_file() {
        setup("browser_state_wipe").await;
        insert_file_from_string("a.cad", "x".into())
            .await
            .expect("insert");
        insert_file_from_string("b.cad", "y".into())
            .await
            .expect("insert");

        wipe_state();

        assert_eq!(FILE_STATE.with_borrow(|state| state.len()), 0);
    }

    #[wasm_bindgen_test]
    async fn write_transaction_applies_and_persists() {
        setup("browser_state_write_tx").await;
        insert_file_from_string("a.cad", "world".into())
            .await
            .expect("insert");

        write_transaction_to_file("a.cad", &insert_at(0, 0, "hello "))
            .await
            .expect("apply");

        assert_eq!(read_file("a.cad").as_deref(), Some("hello world"));
        // The post-edit text reached IndexedDB for the compiler.
        assert_eq!(
            read_store("browser_state_write_tx", "a.cad")
                .await
                .as_deref(),
            Some("hello world")
        );
    }

    #[wasm_bindgen_test]
    async fn write_transaction_to_missing_file_errors() {
        setup("browser_state_write_missing").await;

        let result = write_transaction_to_file("ghost.cad", &insert_at(0, 0, "x")).await;

        assert!(result.is_err());
    }

    #[wasm_bindgen_test]
    async fn apply_update_merges_a_peer_edit() {
        setup("browser_state_apply_update").await;
        // Two replicas of the same file, joined from one origin state so their CRDT identities
        // line up and the update converges.
        let origin_store = IndexedDbIo::new("browser_state_apply_origin")
            .await
            .unwrap();
        let origin = MemFile::from_string(file("a.cad"), Async::new(&origin_store), "world".into());
        insert_file_from_state("a.cad", &origin.encode_state())
            .await
            .expect("join");
        assert_eq!(read_file("a.cad").as_deref(), Some("world"));

        // The origin makes an edit and ships the diff; our buffer merges it.
        let mut origin = origin;
        let state_vector = origin.state_vector();
        origin
            .insert_text(CursorIndex { line: 0, col: 0 }, "hello ")
            .await
            .unwrap();
        let update = origin.encode_diff(&state_vector).unwrap();

        apply_update_to_file("a.cad", &update)
            .await
            .expect("apply update");

        assert_eq!(read_file("a.cad").as_deref(), Some("hello world"));
    }

    /// Opens `a.cad` in the session as a replica of an origin buffer holding `start`, and
    /// returns two updates the origin makes one after the other: inserting `first` at the
    /// start, then `second` at the start. Applying both, in order, gives `second + first +
    /// start`.
    async fn joined_with_two_updates(
        origin_db: &str,
        start: &str,
        first: &str,
        second: &str,
    ) -> (Vec<u8>, Vec<u8>) {
        let origin_store = IndexedDbIo::new(origin_db).await.unwrap();
        let mut origin =
            MemFile::from_string(file("a.cad"), Async::new(&origin_store), start.into());
        insert_file_from_state("a.cad", &origin.encode_state())
            .await
            .expect("join");

        let before_first = origin.state_vector();
        origin
            .insert_text(CursorIndex { line: 0, col: 0 }, first)
            .await
            .unwrap();
        let first_update = origin.encode_diff(&before_first).unwrap();

        let before_second = origin.state_vector();
        origin
            .insert_text(CursorIndex { line: 0, col: 0 }, second)
            .await
            .unwrap();
        let second_update = origin.encode_diff(&before_second).unwrap();

        (first_update, second_update)
    }

    #[wasm_bindgen_test]
    async fn back_to_back_updates_both_merge_while_a_save_is_in_flight() {
        // The case a compile produces: the server releases a queue of held echoes together,
        // so a second update arrives while the first one's IndexedDB write is still pending.
        // `join!` polls the first merge until it awaits its write, then starts the second.
        // Taking the buffer out of the map for the write used to fail the second merge as
        // "not open" and lose its update.
        setup("browser_state_back_to_back").await;
        let (first, second) = joined_with_two_updates(
            "browser_state_back_to_back_origin",
            "world",
            "hello ",
            ">> ",
        )
        .await;

        let (first_result, second_result) = join!(
            apply_update_to_file("a.cad", &first),
            apply_update_to_file("a.cad", &second)
        );

        assert!(first_result.is_ok());
        assert!(second_result.is_ok());
        assert_eq!(read_file("a.cad").as_deref(), Some(">> hello world"));
        // The overlapping writes landed in order, so IndexedDB holds the newest text.
        assert_eq!(
            read_store("browser_state_back_to_back", "a.cad")
                .await
                .as_deref(),
            Some(">> hello world")
        );
    }

    #[wasm_bindgen_test]
    async fn read_file_sees_the_merge_while_its_save_is_in_flight() {
        // The editor reconciles by reading the buffer, and it can do so while a save is still
        // pending. The buffer has to be there, already merged, rather than missing.
        setup("browser_state_read_during_save").await;
        let (first, _) = joined_with_two_updates(
            "browser_state_read_during_save_origin",
            "world",
            "hello ",
            "",
        )
        .await;

        let (result, read_during_save) = join!(apply_update_to_file("a.cad", &first), async {
            read_file("a.cad")
        });

        assert!(result.is_ok());
        assert_eq!(read_during_save.as_deref(), Some("hello world"));
    }

    #[wasm_bindgen_test]
    async fn flush_all_keeps_files_open_while_it_writes() {
        // A compile flushes every buffer first. Files used to be taken out of the map for the
        // whole flush, so any merge during it failed and every read came back empty.
        setup("browser_state_flush_keeps_open").await;
        let (first, _) = joined_with_two_updates(
            "browser_state_flush_keeps_open_origin",
            "world",
            "hello ",
            "",
        )
        .await;

        let (flush_result, merge_result) =
            join!(flush_all(), apply_update_to_file("a.cad", &first));

        assert!(flush_result.is_ok());
        assert!(merge_result.is_ok());
        assert_eq!(read_file("a.cad").as_deref(), Some("hello world"));
        assert_eq!(
            read_store("browser_state_flush_keeps_open", "a.cad")
                .await
                .as_deref(),
            Some("hello world")
        );
    }

    #[wasm_bindgen_test]
    async fn read_file_missing_returns_none() {
        setup("browser_state_read_missing").await;

        assert!(read_file("ghost.cad").is_none());
    }

    #[wasm_bindgen_test]
    async fn a_merge_lands_while_a_transaction_save_is_in_flight() {
        // A local transaction's save and a peer's update overlap. The transaction used to take
        // the buffer out of the map for its save, so the merge failed as "not open".
        setup("browser_state_tx_then_merge").await;
        let (first, _) =
            joined_with_two_updates("browser_state_tx_then_merge_origin", "world", "hello ", "")
                .await;

        // A bulk insert at the end, so it is saved straight away and does not collide with
        // the peer's insert at the start.
        let transaction = insert_at(0, 5, "!!");
        let (write_result, merge_result) = join!(
            write_transaction_to_file("a.cad", &transaction),
            apply_update_to_file("a.cad", &first)
        );

        assert!(write_result.is_ok());
        assert!(merge_result.is_ok());
        assert_eq!(read_file("a.cad").as_deref(), Some("hello world!!"));
        assert_eq!(
            read_store("browser_state_tx_then_merge", "a.cad")
                .await
                .as_deref(),
            Some("hello world!!")
        );
    }

    #[wasm_bindgen_test]
    async fn single_character_transactions_save_once_the_threshold_is_reached() {
        setup("browser_state_tx_threshold").await;
        insert_file_from_string("a.cad", String::new())
            .await
            .expect("insert");

        // Below the threshold the edits stay in the buffer only.
        for column in 0..SAVE_THRESHOLD - 1 {
            write_transaction_to_file("a.cad", &insert_at(0, column, "x"))
                .await
                .expect("edit");
        }
        assert_eq!(
            read_store("browser_state_tx_threshold", "a.cad")
                .await
                .as_deref(),
            Some("")
        );

        // The edit that reaches the threshold saves all of them.
        write_transaction_to_file("a.cad", &insert_at(0, SAVE_THRESHOLD - 1, "x"))
            .await
            .expect("edit");
        let expected = "x".repeat(SAVE_THRESHOLD);
        assert_eq!(
            read_store("browser_state_tx_threshold", "a.cad")
                .await
                .as_deref(),
            Some(expected.as_str())
        );
    }

    #[wasm_bindgen_test]
    async fn a_mixed_transaction_saves_its_final_text_once() {
        // A bulk edit followed by single character edits in one transaction: the whole
        // transaction is applied before its one save, so IndexedDB gets the final text,
        // trailing single characters included.
        setup("browser_state_tx_mixed").await;
        insert_file_from_string("a.cad", "world".into())
            .await
            .expect("insert");
        let transaction = Transaction::new(vec![
            Operation::Insert(InsertSlice {
                position: CursorIndex { line: 0, col: 0 },
                content: "hello ".into(),
            }),
            Operation::Insert(InsertSlice {
                position: CursorIndex { line: 0, col: 11 },
                content: "!".into(),
            }),
            Operation::Swap(SwapSlice {
                position: CursorIndex { line: 0, col: 0 },
                length: 1,
                content: "H".into(),
            }),
        ]);

        write_transaction_to_file("a.cad", &transaction)
            .await
            .expect("apply");

        assert_eq!(read_file("a.cad").as_deref(), Some("Hello world!"));
        assert_eq!(
            read_store("browser_state_tx_mixed", "a.cad")
                .await
                .as_deref(),
            Some("Hello world!")
        );
    }

    #[wasm_bindgen_test]
    async fn flush_all_persists_batched_edits() {
        setup("browser_state_flush").await;
        insert_file_from_string("a.cad", "abcd".into())
            .await
            .expect("insert");

        // A single char edit stays batched below the save threshold, so IndexedDB still holds
        // the seeded text until an explicit flush.
        write_transaction_to_file("a.cad", &insert_at(0, 0, "X"))
            .await
            .expect("edit");
        assert_eq!(
            read_store("browser_state_flush", "a.cad").await.as_deref(),
            Some("abcd")
        );

        flush_all().await.expect("flush");

        assert_eq!(
            read_store("browser_state_flush", "a.cad").await.as_deref(),
            Some("Xabcd")
        );
    }

    /// Opens a set of files under the given paths for the subpath tests.
    async fn open(paths: &[&str]) {
        for path in paths {
            insert_file_from_string(path, String::new())
                .await
                .expect("open");
        }
    }

    #[wasm_bindgen_test]
    async fn subpaths_of_returns_files_under_the_folder_sorted() {
        setup("browser_state_subpaths").await;
        open(&["src/util/lib.cad", "src/main.cad", "readme.md"]).await;

        assert_eq!(subpaths_of("src"), vec!["src/main.cad", "src/util/lib.cad"]);
    }

    #[wasm_bindgen_test]
    async fn subpaths_of_matches_on_component_boundaries() {
        setup("browser_state_subpaths_components").await;
        open(&["src/main.cad", "src2/main.cad", "source/main.cad"]).await;

        // A shared string prefix (`src2`, `source`) is not a shared path prefix.
        assert_eq!(subpaths_of("src"), vec!["src/main.cad"]);
    }

    #[wasm_bindgen_test]
    async fn subpaths_of_empty_path_matches_everything() {
        setup("browser_state_subpaths_root").await;
        open(&["a.cad", "src/b.cad"]).await;

        assert_eq!(subpaths_of(""), vec!["a.cad", "src/b.cad"]);
    }
}
