//! Serve a production build of a Sap scaffolded frontend (`frontend/web/public` after `vite build`),
//! embedded at compile time via [`include_dir::include_dir`] in your application crate.
//!
//! Behaviour: correct `Content-Type` (including Wasm), cache headers, `/api/*` left alone (404 here
//! so API routers can take precedence when composed), and SPA fallback to `index.html` for
//! extensionless paths.
//!
//! # Wiring (without `axum::extract::State`)
//!
//! [`Frontend`] is [`Copy`]. Handlers close over it, so the router stays `Router<()>`
//! (default Axum state): register API routes first, then attach the UI.
//!
//! ```ignore
//! use saps::axum::{Router, routing::get};
//! use saps::frontend::Frontend;
//!
//! static PUBLIC: include_dir::Dir<'static> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/frontend/web/public");
//! let public = Frontend::new(&PUBLIC);
//!
//! let app = Router::new().route("/health", get(|| async { "ok" }));
//! let app = public.attach_to_router(app);
//! ```
//!
//! For UI-only, [`Frontend::into_router`] is [`Frontend::attach_to_router`] on an empty router.

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, HeaderValue, Method, Request, StatusCode, Uri, header},
    response::Response,
    routing::{any, get},
};
use mime_guess::MimeGuess;

/// Serves static files from a compile-time embedded tree (typically the Vite `public` output dir).
///
/// Build the tree with [`include_dir::include_dir!`] in your binary crate so paths resolve against
/// that crate’s manifest.
#[derive(Clone, Copy, Debug)]
pub struct Frontend {
    dir: &'static include_dir::Dir<'static>,
}

impl Frontend {
    pub const fn new(dir: &'static include_dir::Dir<'static>) -> Self {
        Self { dir }
    }

    /// Adds `GET /` and a catch-all fallback for static files / SPA (router state stays `()`).
    pub fn attach_to_router(self, app: Router) -> Router {
        app.route(
            "/",
            get(move || async move { self.serve_uri(Uri::from_static("/")).await }),
        )
        .fallback(any(move |req: Request<Body>| async move {
            self.serve_request(req).await
        }))
    }

    /// [`Router::new()`] with only `/` + fallback (same as [`Self::attach_to_router`] on empty).
    pub fn into_router(self) -> Router {
        self.attach_to_router(Router::new())
    }

    /// Full HTTP handling for static / SPA (GET body, HEAD without body, other methods → 405).
    pub async fn serve_request(&self, req: Request<Body>) -> Response<Body> {
        match *req.method() {
            Method::GET => self.serve_uri(req.uri().clone()).await,
            Method::HEAD => {
                let mut res = self.serve_uri(req.uri().clone()).await;
                *res.body_mut() = Body::empty();
                res
            }
            _ => text_response(StatusCode::METHOD_NOT_ALLOWED, "Method Not Allowed"),
        }
    }

    /// Map a URI to a response (GET semantics only; prefer [`Self::serve_request`] for HEAD).
    pub async fn serve_uri(&self, uri: Uri) -> Response<Body> {
        let path = uri.path();

        if path.starts_with("/api/") {
            return text_response(StatusCode::NOT_FOUND, "Not Found");
        }

        let rel = path.trim_start_matches('/');

        if rel.is_empty() {
            return self.index_response();
        }

        if !is_safe_rel_path(rel) {
            let looks_like_file = rel.rsplit_once('.').is_some();
            return if looks_like_file {
                text_response(StatusCode::NOT_FOUND, "404 Not Found")
            } else {
                self.index_response()
            };
        }

        if let Some(resp) = self.file_response(rel) {
            return resp;
        }

        let looks_like_file = rel.rsplit_once('.').is_some();
        if looks_like_file {
            return text_response(StatusCode::NOT_FOUND, "404 Not Found");
        }

        self.index_response()
    }

    fn index_response(&self) -> Response<Body> {
        match self.file_response("index.html") {
            Some(resp) => resp,
            None => text_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "index.html missing from embedded public directory",
            ),
        }
    }

    fn file_response(&self, relative: &str) -> Option<Response<Body>> {
        let file = self.dir.get_file(relative)?;
        let bytes = file.contents().to_vec();
        Some(ok_file_response(bytes, relative))
    }
}

fn ok_file_response(bytes: Vec<u8>, path_for_mime: &str) -> Response<Body> {
    let content_type = if path_for_mime.ends_with(".wasm") {
        HeaderValue::from_static("application/wasm")
    } else {
        let mime = MimeGuess::from_path(path_for_mime)
            .first_or_octet_stream()
            .to_string();
        HeaderValue::from_str(&mime)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"))
    };

    let cache_control = if path_for_mime == "index.html" {
        HeaderValue::from_static("no-cache")
    } else {
        HeaderValue::from_static("public, max-age=604800")
    };

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type);
    headers.insert(header::CACHE_CONTROL, cache_control);

    Response::builder()
        .status(StatusCode::OK)
        .body(Body::from(bytes))
        .map(|r| apply_headers(r, headers))
        .unwrap_or_else(|_| {
            text_response(StatusCode::INTERNAL_SERVER_ERROR, "response build error")
        })
}

fn text_response(status: StatusCode, body: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(Body::from(body))
        .unwrap()
}

fn apply_headers(mut response: Response<Body>, headers: HeaderMap) -> Response<Body> {
    *response.headers_mut() = headers;
    response
}

fn is_safe_rel_path(url_path: &str) -> bool {
    for seg in url_path.split('/').filter(|s| !s.is_empty()) {
        if seg == ".." || seg.contains('\0') || seg.contains('\\') {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    static FIXTURE: include_dir::Dir<'static> =
        include_dir::include_dir!("$CARGO_MANIFEST_DIR/tests/fixtures/embed_public");

    #[tokio::test]
    async fn serves_index_and_nested_asset() {
        let dir = Frontend::new(&FIXTURE);
        let r = dir.serve_uri(Uri::from_static("/")).await;
        assert_eq!(r.status(), StatusCode::OK);
        let body = r.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(std::str::from_utf8(&body).unwrap().trim(), "<html></html>");

        let r = dir.serve_uri(Uri::from_static("/assets/x.svg")).await;
        assert_eq!(r.status(), StatusCode::OK);
        let body = r.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(std::str::from_utf8(&body).unwrap().trim(), "<svg/>");
    }

    #[tokio::test]
    async fn spa_fallback_for_extensionless_path() {
        let dir = Frontend::new(&FIXTURE);
        let r = dir.serve_uri(Uri::from_static("/dashboard/settings")).await;
        assert_eq!(r.status(), StatusCode::OK);
        let body = r.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(std::str::from_utf8(&body).unwrap().trim(), "<html></html>");
    }

    #[tokio::test]
    async fn missing_asset_with_extension_is_404() {
        let dir = Frontend::new(&FIXTURE);
        let r = dir.serve_uri(Uri::from_static("/assets/missing.svg")).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn api_prefix_returns_404_from_spa_layer() {
        let dir = Frontend::new(&FIXTURE);
        let r = dir.serve_uri(Uri::from_static("/api/v1/x")).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let dir = Frontend::new(&FIXTURE);
        let r = dir.serve_uri(Uri::from_static("/../secret.txt")).await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
    }
}
