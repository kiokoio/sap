//! The async write path for the typestate `MemFile`.
//!
//! These are the editing methods for `MemFile<Async<..>>`: the awaited twin of
//! [`tx_blocking`](super::tx_blocking). Each verb applies its edit through the same shared
//! unsaved verbs in [`state`](super::state) — the edit and the saving policy are identical and
//! colour-free — and differs only in the save, which is the awaited
//! [`AsyncFileIo::write_file`]. That is the whole reason the two write paths cannot be one
//! method: the save is coloured, so everything that triggers it must be too.
//!
//! ## Saving policy (unchanged from the server `MemFile`)
//!
//! - Single character edits (`insert_char`, `delete_char`) are counted and only flushed once
//!   `SAVE_THRESHOLD` of them have accumulated since the last save.
//! - Bulk edits (`delete_range`, `insert_text`) flush immediately, because a single one can
//!   change a lot of text at once.
//! - `apply_update` (a remote edit off the relay) is not flushed here — and, since applying
//!   an update is synchronous, it stays a non-`async` method even on this path.
use crate::errors::file::FileError;
use crate::kernel::transaction::CursorIndex;

use crate::files::files::mem_files::full_descriptors::tx_utils;
use crate::files::files::mem_files::full_mem_file::{Async, MemFile};
use crate::files::io::async_file::AsyncFileIo;

impl<'a, S: AsyncFileIo> MemFile<Async<'a, S>> {
    /// Inserts a single character at a cursor position. Per key stroke.
    ///
    /// The common typing path: applied to the buffer and only flushed once enough of these
    /// have built up (see `SAVE_THRESHOLD`).
    ///
    /// # Arguments
    /// - `position`: Where in the buffer to insert, as a line and column.
    /// - `character`: The character to insert.
    ///
    /// # Returns
    /// `Ok(())` once the character is inserted, or a `FileError` if crossing the threshold
    /// triggered a save that the backend rejected.
    pub async fn insert_char(
        &mut self,
        position: CursorIndex,
        character: char,
    ) -> Result<(), FileError> {
        if self.insert_char_unsaved(position, character)? {
            self.save().await?;
        }
        Ok(())
    }

    /// Deletes the single character at a cursor position. Per key stroke.
    ///
    /// Like `insert_char` this is the common path and so is only flushed once
    /// `SAVE_THRESHOLD` single character edits have built up. A position at or past the end
    /// of the buffer is a no-op rather than a panic.
    ///
    /// # Arguments
    /// - `position`: The position of the character to delete, as a line and column.
    ///
    /// # Returns
    /// `Ok(())` once the character is removed, or a `FileError` if crossing the threshold
    /// triggered a save that the backend rejected.
    pub async fn delete_char(&mut self, position: CursorIndex) -> Result<(), FileError> {
        if self.delete_char_unsaved(position)? {
            self.save().await?;
        }
        Ok(())
    }

    /// Deletes a run of characters starting at a cursor position.
    ///
    /// The "highlighted a range and deleted it" path. A single one of these can remove a lot
    /// of text, so it is saved straight away. The run is clamped to the characters actually
    /// available from `position`, so an over-long `delta` removes to the end rather than
    /// panicking.
    ///
    /// # Arguments
    /// - `position`: The start of the range to delete, as a line and column.
    /// - `delta`: The number of characters to remove from `position` onwards.
    ///
    /// # Returns
    /// `Ok(())` once the run is removed and flushed, or a `FileError` if the backend rejected
    /// the save.
    pub async fn delete_range(
        &mut self,
        position: CursorIndex,
        delta: usize,
    ) -> Result<(), FileError> {
        self.delete_range_unsaved(position, delta)?;
        self.save().await
    }

    /// Inserts a run of text at a cursor position.
    ///
    /// The batched / paste path. A single one of these can add a lot of text, so it is saved
    /// straight away rather than counted towards `SAVE_THRESHOLD`.
    ///
    /// # Arguments
    /// - `position`: Where to insert the text, as a line and column.
    /// - `data`: The text to insert.
    ///
    /// # Returns
    /// `Ok(())` once the text is inserted and flushed, or a `FileError` if the backend
    /// rejected the save.
    pub async fn insert_text(
        &mut self,
        position: CursorIndex,
        data: &str,
    ) -> Result<(), FileError> {
        self.insert_text_unsaved(position, data)?;
        self.save().await
    }

    /// Applies a `yrs` update from another replica (for example one fanned out by the relay)
    /// to this buffer.
    ///
    /// Updates are idempotent and commutative, so applying the same update twice or applying
    /// updates out of order still converges. Applying an update is synchronous and the buffer
    /// is *not* flushed here, so unlike the other verbs this one stays non-`async` — the
    /// caller decides when a run of relayed edits is worth an awaited `save`.
    ///
    /// # Arguments
    /// - `update`: A `yrs` v1 update blob, as produced by `encode_diff` or another replica's
    ///   update observer.
    ///
    /// # Returns
    /// `Ok(())` once applied, or a `FileError::MemFile` if the update cannot be decoded or
    /// applied.
    pub fn apply_update(&mut self, update: &[u8]) -> Result<(), FileError> {
        tx_utils::apply_update(&mut self.doc, &self.path, update)
    }
}

#[cfg(test)]
mod tests {
    // Mirror the blocking tests over an `AsyncMemIo`, driven to completion with `block_on`, so
    // the same batching policy is checked on the awaited write path. Reading the store back is
    // what proves what actually reached the backend.

    use super::*;
    use crate::files::engines::async_io::mem::AsyncMemIo;
    use crate::files::paths::{FilePath, Path};
    use futures::executor::block_on;

    const START: &str = "the cat sat on the mat";

    fn cursor(line: usize, col: usize) -> CursorIndex {
        CursorIndex { line, col }
    }

    fn file(name: &str) -> Path<FilePath> {
        Path::<FilePath>::new(name).unwrap()
    }

    /// Seeds the store with `START` at `main.cad` and returns it, so `from_file` has something
    /// to load and later assertions have a known baseline to compare the backend against.
    async fn seeded_store() -> AsyncMemIo {
        let store = AsyncMemIo::new();
        store.write_file(&file("main.cad"), START).await.unwrap();
        store
    }

    /// Reads `main.cad` straight out of the store and asserts its whole contents.
    async fn assert_stored(store: &AsyncMemIo, expected: &str) {
        assert_eq!(store.read_file(&file("main.cad")).await.unwrap(), expected);
    }

    #[test]
    fn insert_char_batches_until_threshold() {
        block_on(async {
            let store = seeded_store().await;
            let mut mem = MemFile::<Async<'_, _>>::from_file(file("main.cad"), Async::new(&store))
                .await
                .unwrap();

            // Four inserts stay in memory; the backend is untouched.
            for _ in 0..4 {
                mem.insert_char(cursor(0, 0), 'X').await.unwrap();
            }
            assert_eq!(mem.contents(), "XXXXthe cat sat on the mat");
            assert_stored(&store, START).await;

            // The fifth insert crosses the threshold and flushes all five.
            mem.insert_char(cursor(0, 0), 'X').await.unwrap();
            assert_stored(&store, "XXXXXthe cat sat on the mat").await;
        });
    }

    #[test]
    fn delete_char_batches_until_threshold() {
        block_on(async {
            let store = seeded_store().await;
            let mut mem = MemFile::<Async<'_, _>>::from_file(file("main.cad"), Async::new(&store))
                .await
                .unwrap();

            for _ in 0..4 {
                mem.delete_char(cursor(0, 0)).await.unwrap();
            }
            assert_stored(&store, START).await;

            // The fifth delete flushes. Five chars removed from the front: "the c".
            mem.delete_char(cursor(0, 0)).await.unwrap();
            assert_stored(&store, "at sat on the mat").await;
        });
    }

    #[test]
    fn delete_range_saves_immediately() {
        block_on(async {
            let store = seeded_store().await;
            let mut mem = MemFile::<Async<'_, _>>::from_file(file("main.cad"), Async::new(&store))
                .await
                .unwrap();

            mem.delete_range(cursor(0, 0), 4).await.unwrap();
            assert_stored(&store, "cat sat on the mat").await;
        });
    }

    #[test]
    fn insert_text_saves_immediately() {
        block_on(async {
            let store = seeded_store().await;
            let mut mem = MemFile::<Async<'_, _>>::from_file(file("main.cad"), Async::new(&store))
                .await
                .unwrap();

            mem.insert_text(cursor(0, 4), "fat ").await.unwrap();
            assert_stored(&store, "the fat cat sat on the mat").await;
        });
    }

    #[test]
    fn apply_update_applies_without_flushing() {
        block_on(async {
            // The joiner adopts the origin's CRDT identities via `from_state` (independent
            // `from_file`s of the same bytes would not converge), then a relayed edit is
            // applied. Applying is synchronous and does not flush, so the joiner's own store
            // still holds its baseline until an explicit awaited `save`.
            let origin_store = seeded_store().await;
            let origin =
                MemFile::<Async<'_, _>>::from_file(file("main.cad"), Async::new(&origin_store))
                    .await
                    .unwrap();

            let mem_store = seeded_store().await;
            let mut mem =
                MemFile::from_state(file("main.cad"), &origin.state(), Async::new(&mem_store))
                    .unwrap();
            assert_eq!(mem.contents(), START);

            // The origin makes a bulk edit (flushing to its own store) and ships only the delta.
            let before = mem.state_vector();
            let mut origin = origin;
            origin.insert_text(cursor(0, 0), "hello ").await.unwrap();
            let update = origin.encode_diff(&before).unwrap();

            mem.apply_update(&update).unwrap();
            assert_eq!(mem.contents(), "hello the cat sat on the mat");
            // The apply did not touch the joiner's store — it still holds the seeded baseline.
            assert_stored(&mem_store, START).await;

            mem.save().await.unwrap();
            assert_stored(&mem_store, "hello the cat sat on the mat").await;
        });
    }
}
