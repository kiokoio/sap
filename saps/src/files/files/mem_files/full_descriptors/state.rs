//! The state functions for the typestate `MemFile`.
use crate::errors::file::FileError;
use crate::kernel::transaction::CursorIndex;

use crate::files::files::mem_files::constants::SAVE_THRESHOLD;
use crate::files::files::mem_files::full_descriptors::tx_utils;
use crate::files::files::mem_files::full_mem_file::{Backend, MemFile};
use yrs::{
    GetString, ReadTxn, StateVector, Transact,
    updates::{decoder::Decode, encoder::Encode},
};

// MARK: - Generic impl

impl<B: Backend> MemFile<B> {
    /// The current text of the buffer, including any unsaved edits.
    ///
    /// This is the materialised view the LSP / compiler reads and the exact bytes
    /// `save` writes to the backend. It is also what makes line and column
    /// addressing work: the offset helpers resolve positions against this string.
    pub fn contents(&self) -> String {
        self.text.get_string(&self.doc.transact())
    }

    /// Encodes the whole document as a `yrs` state update.
    ///
    /// This is what a *joining* replica is seeded with via [`MemFile::from_state`],
    /// and what you persist as a durable CRDT snapshot so a reloaded session can
    /// rebuild the document (not just the flat text) and keep merging.
    pub fn encode_state(&self) -> Vec<u8> {
        self.doc
            .transact()
            .encode_state_as_update_v1(&StateVector::default())
    }

    /// Encodes this replica's state vector — a compact summary of which edits it
    /// already has.
    ///
    /// A joining replica sends this to the origin, which replies with
    /// [`MemFile::encode_diff`] carrying only the edits this replica is missing,
    /// so catch-up does not replay the whole history.
    pub fn state_vector(&self) -> Vec<u8> {
        self.doc.transact().state_vector().encode_v1()
    }

    /// Encodes only the edits missing from a peer with the given `state_vector`.
    ///
    /// The other half of the joining handshake: given a peer's
    /// [`MemFile::state_vector`], produce the minimal update that brings it up to
    /// date.
    pub fn encode_diff(&self, state_vector: &[u8]) -> Result<Vec<u8>, FileError> {
        let remote = StateVector::decode_v1(state_vector).map_err(|error| FileError::MemFile {
            path: self.path.relative_string(),
            message: error.to_string(),
        })?;
        Ok(self.doc.transact().encode_diff_v1(&remote))
    }
}

// MARK: - Unsaved edits

/// The editing verbs without the save, written once for every backend.
///
/// Each applies its edit to the document, applies the saving policy's bookkeeping, and
/// returns whether the buffer is now due a save — leaving the save itself to the caller,
/// because the save is the part coloured blocking or async. The blocking and async verbs in
/// [`tx_blocking`](super::tx_blocking) and [`tx_async`](super::tx_async) save when told to;
/// the browser session applies a whole transaction in place and saves once at the end. The
/// policy lives only here:
///
/// - Single character edits count towards [`SAVE_THRESHOLD`] and are due a save once that
///   many have built up since the last one.
/// - Bulk edits (a run of text) are always due a save, because one can change a lot of text.
impl<B: Backend> MemFile<B> {
    /// Inserts a single character at a cursor position without saving. Per key stroke.
    ///
    /// # Returns
    /// Whether [`SAVE_THRESHOLD`] single character edits have now built up.
    pub fn insert_char_unsaved(
        &mut self,
        position: CursorIndex,
        character: char,
    ) -> Result<bool, FileError> {
        let contents = self.contents();
        tx_utils::insert_char(
            &contents,
            &mut self.text,
            &mut self.doc,
            &position,
            character,
        )?;
        Ok(self.count_single_edit())
    }

    /// Deletes the single character at a cursor position without saving. Per key stroke. A
    /// position at or past the end of the buffer is a no-op rather than a panic.
    ///
    /// # Returns
    /// Whether [`SAVE_THRESHOLD`] single character edits have now built up.
    pub fn delete_char_unsaved(&mut self, position: CursorIndex) -> Result<bool, FileError> {
        let contents = self.contents();
        tx_utils::delete_char(&contents, &mut self.text, &mut self.doc, &position)?;
        Ok(self.count_single_edit())
    }

    /// Deletes a run of characters starting at a cursor position without saving. The run is
    /// clamped to the characters available from `position`.
    ///
    /// # Returns
    /// Always `true`: a bulk edit is always due a save.
    pub fn delete_range_unsaved(
        &mut self,
        position: CursorIndex,
        delta: usize,
    ) -> Result<bool, FileError> {
        let contents = self.contents();
        tx_utils::delete_range(&contents, &mut self.text, &mut self.doc, &position, delta)?;
        Ok(true)
    }

    /// Inserts a run of text at a cursor position without saving.
    ///
    /// # Returns
    /// Always `true`: a bulk edit is always due a save.
    pub fn insert_text_unsaved(
        &mut self,
        position: CursorIndex,
        data: &str,
    ) -> Result<bool, FileError> {
        let contents = self.contents();
        tx_utils::insert_text(&contents, &mut self.text, &mut self.doc, &position, data)?;
        Ok(true)
    }

    /// Counts a single character edit and reports whether [`SAVE_THRESHOLD`] have built up
    /// since the last save. The save that follows resets the counter.
    fn count_single_edit(&mut self) -> bool {
        self.ops_since_save += 1;
        self.ops_since_save >= SAVE_THRESHOLD
    }
}
