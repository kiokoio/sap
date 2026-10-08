//! The blocking write path for the typestate `MemFile`.
//!
//! These are the editing methods for `MemFile<Blocking<..>>`: the user-facing verbs the
//! server (or a blocking-backed client) calls. Each one applies its edit through the shared
//! unsaved verbs in [`state`](super::state), which also apply the saving policy and say
//! whether a save is due, and then saves when it is. The save is the synchronous
//! [`FileIo::write_file`], so this whole surface is coloured blocking; its awaited twin lives
//! in [`tx_async`](super::tx_async).
//!
//! ## Saving policy (unchanged from the server `MemFile`)
//!
//! - Single character edits (`insert_char`, `delete_char`) are counted and only flushed once
//!   `SAVE_THRESHOLD` of them have accumulated since the last save.
//! - Bulk edits (`delete_range`, `insert_text`) flush immediately, because a single one can
//!   change a lot of text at once.
//! - `apply_update` (a remote edit off the relay) is not flushed here — the caller decides
//!   when a run of relayed edits is worth a save, keeping remote edits on the same batching
//!   policy as local ones.
use crate::errors::file::FileError;
use crate::kernel::transaction::CursorIndex;

use crate::files::files::mem_files::full_descriptors::tx_utils;
use crate::files::files::mem_files::full_mem_file::{Blocking, MemFile};
use crate::files::io::file::FileIo;

impl<'a, S: FileIo> MemFile<Blocking<'a, S>> {
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
    pub fn insert_char(&mut self, position: CursorIndex, character: char) -> Result<(), FileError> {
        if self.insert_char_unsaved(position, character)? {
            self.save()?;
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
    pub fn delete_char(&mut self, position: CursorIndex) -> Result<(), FileError> {
        if self.delete_char_unsaved(position)? {
            self.save()?;
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
    pub fn delete_range(&mut self, position: CursorIndex, delta: usize) -> Result<(), FileError> {
        self.delete_range_unsaved(position, delta)?;
        self.save()
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
    pub fn insert_text(&mut self, position: CursorIndex, data: &str) -> Result<(), FileError> {
        self.insert_text_unsaved(position, data)?;
        self.save()
    }

    /// Applies a `yrs` update from another replica (for example one fanned out by the relay)
    /// to this buffer.
    ///
    /// Updates are idempotent and commutative, so applying the same update twice or applying
    /// updates out of order still converges. The buffer is *not* flushed here — the caller
    /// decides when a run of relayed edits is worth a `save`.
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
    // Each test builds a `MemFile<Blocking<..>>` over a `BlockingMemIo` seeded with a known
    // file, drives one editing verb, and reads the store back to assert what actually reached
    // the backend — so the assertions check the batching policy, not just the in-memory text.

    use super::*;
    use crate::files::engines::blocking_io::mem::BlockingMemIo;
    use crate::files::paths::{FilePath, Path};

    const START: &str = "the cat sat on the mat";

    fn cursor(line: usize, col: usize) -> CursorIndex {
        CursorIndex { line, col }
    }

    fn file(name: &str) -> Path<FilePath> {
        Path::<FilePath>::new(name).unwrap()
    }

    /// Seeds the store with `START` at `main.cad` and returns it, so `from_file` has something
    /// to load and later assertions have a known baseline to compare the backend against.
    fn seeded_store() -> BlockingMemIo {
        let store = BlockingMemIo::new();
        store.write_file(&file("main.cad"), START).unwrap();
        store
    }

    /// Reads `main.cad` straight out of the store and asserts its whole contents, so every
    /// assertion checks what the backend actually persisted.
    fn assert_stored(store: &BlockingMemIo, expected: &str) {
        assert_eq!(store.read_file(&file("main.cad")).unwrap(), expected);
    }

    #[test]
    fn insert_char_batches_until_threshold() {
        let store = seeded_store();
        let mut mem =
            MemFile::<Blocking<'_, _>>::from_file(file("main.cad"), Blocking::new(&store)).unwrap();

        // Four inserts is below the threshold of five, so the backend is untouched even though
        // the in-memory buffer already holds them.
        for _ in 0..4 {
            mem.insert_char(cursor(0, 0), 'X').unwrap();
        }
        assert_eq!(mem.contents(), "XXXXthe cat sat on the mat");
        assert_stored(&store, START);

        // The fifth insert crosses the threshold and flushes all five.
        mem.insert_char(cursor(0, 0), 'X').unwrap();
        assert_stored(&store, "XXXXXthe cat sat on the mat");
    }

    #[test]
    fn delete_char_batches_until_threshold() {
        let store = seeded_store();
        let mut mem =
            MemFile::<Blocking<'_, _>>::from_file(file("main.cad"), Blocking::new(&store)).unwrap();

        // Four deletes from the front stay in memory; the backend is untouched.
        for _ in 0..4 {
            mem.delete_char(cursor(0, 0)).unwrap();
        }
        assert_stored(&store, START);

        // The fifth delete flushes. Five chars removed from the front: "the c".
        mem.delete_char(cursor(0, 0)).unwrap();
        assert_stored(&store, "at sat on the mat");
    }

    #[test]
    fn delete_range_saves_immediately() {
        let store = seeded_store();
        let mut mem =
            MemFile::<Blocking<'_, _>>::from_file(file("main.cad"), Blocking::new(&store)).unwrap();

        mem.delete_range(cursor(0, 0), 4).unwrap();
        assert_stored(&store, "cat sat on the mat");
    }

    #[test]
    fn insert_text_saves_immediately() {
        let store = seeded_store();
        let mut mem =
            MemFile::<Blocking<'_, _>>::from_file(file("main.cad"), Blocking::new(&store)).unwrap();

        mem.insert_text(cursor(0, 4), "fat ").unwrap();
        assert_stored(&store, "the fat cat sat on the mat");
    }

    #[test]
    fn edits_resolve_against_line_and_column() {
        let store = BlockingMemIo::new();
        store.write_file(&file("main.cad"), "first line\nsecond line").unwrap();
        let mut mem =
            MemFile::<Blocking<'_, _>>::from_file(file("main.cad"), Blocking::new(&store)).unwrap();

        // Column three on line one is before the 'o' of "second"; a text insert flushes.
        mem.insert_text(cursor(1, 3), "X").unwrap();
        assert_stored(&store, "first line\nsecXond line");
    }

    #[test]
    fn apply_update_applies_without_flushing() {
        // The joiner adopts the origin's CRDT identities via `from_state` (they would not
        // converge if each independently `from_file`'d the same bytes), then a relayed edit is
        // applied to it. `apply_update` itself does not flush: the joiner's own store still
        // holds its baseline until an explicit `save` carries the converged text through.
        let origin_store = seeded_store();
        let origin =
            MemFile::<Blocking<'_, _>>::from_file(file("main.cad"), Blocking::new(&origin_store))
                .unwrap();

        let mem_store = seeded_store();
        let mut mem =
            MemFile::from_state(file("main.cad"), &origin.state(), Blocking::new(&mem_store))
                .unwrap();
        assert_eq!(mem.contents(), START);

        // The origin makes a bulk edit (flushing to its own store) and ships only the delta.
        let before = mem.state_vector();
        let mut origin = origin;
        origin.insert_text(cursor(0, 0), "hello ").unwrap();
        let update = origin.encode_diff(&before).unwrap();

        mem.apply_update(&update).unwrap();
        assert_eq!(mem.contents(), "hello the cat sat on the mat");
        // The apply did not touch the joiner's store — it still holds the seeded baseline.
        assert_stored(&mem_store, START);

        mem.save().unwrap();
        assert_stored(&mem_store, "hello the cat sat on the mat");
    }
}
