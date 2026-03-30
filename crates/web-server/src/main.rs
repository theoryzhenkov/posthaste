mod api;
mod db;

use std::sync::{Arc, Mutex};

use axum::routing::get;
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
}

#[tokio::main]
async fn main() {
    let conn = db::init_db();
    let state = Arc::new(AppState {
        db: Mutex::new(conn),
    });

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::exact(
            "http://localhost:5173".parse().expect("invalid origin"),
        ))
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = Router::new()
        .route("/api/mailboxes", get(api::list_mailboxes))
        .route(
            "/api/mailboxes/{id}/emails",
            get(api::list_emails_in_mailbox),
        )
        .route("/api/emails", get(api::list_all_emails))
        .route("/api/emails/{id}", get(api::get_email))
        .route("/api/threads/{id}", get(api::get_thread))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3001")
        .await
        .expect("failed to bind to port 3001");
    println!("Server running at http://localhost:3001");
    axum::serve(listener, app)
        .await
        .expect("server error");
}
