pub mod constructors;
pub mod state;
pub mod tx_async;
pub mod tx_blocking;
// Visible to the rest of `mem_files` so `browser_state` can apply a transaction in place,
// without IO, while the buffer stays in `FILE_STATE`.
pub(in crate::files::files::mem_files) mod tx_utils;
