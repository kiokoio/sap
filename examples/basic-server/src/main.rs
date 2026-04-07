use saps::axum::{Router, response::IntoResponse, routing::get};
use saps::config::EnvConfig;

#[cfg(feature = "embed")]
mod ingress;

mod api;
pub mod dal;
pub mod migrations;
pub mod roles;

async fn health() -> impl IntoResponse {
    "OK"
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/health", get(health));

    let app = api::networking::users::users_factory::<EnvConfig>(app);

    #[cfg(feature = "embed")]
    let app = ingress::attach_embedded_frontend(app);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to port 3000");

    println!("listening on {}", listener.local_addr().unwrap());

    saps::axum::serve(listener, app)
        .await
        .expect("server error");
}
