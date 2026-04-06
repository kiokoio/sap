use saps::axum::{Router, routing::get, response::IntoResponse};
use saps::config::EnvConfig;

mod api;
pub mod dal;
pub mod migrations;
pub mod roles;

async fn health() -> impl IntoResponse {
    "OK"
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/health", get(health));

    let app = api::networking::users::users_factory::<EnvConfig>(app);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("failed to bind to port 3000");

    println!("listening on {}", listener.local_addr().unwrap());

    saps::axum::serve(listener, app)
        .await
        .expect("server error");
}
