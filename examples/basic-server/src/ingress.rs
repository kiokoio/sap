//! Compile-time embedded SPA (`frontend/web/public` via `include_dir!`).
//!
//! Enable crate feature `embed` to attach these routes; without it, the binary is API-only.

use include_dir::include_dir;
use saps::axum::Router;
use saps::frontend::Frontend;

static FRONTEND: include_dir::Dir<'static> =
    include_dir!("$CARGO_MANIFEST_DIR/frontend/web/public");

/// Serves `GET /` and falls back to embedded static files / SPA shell for other unmatched routes.
pub fn attach_embedded_frontend(app: Router) -> Router {
    Frontend::new(&FRONTEND).attach_to_router(app)
}
