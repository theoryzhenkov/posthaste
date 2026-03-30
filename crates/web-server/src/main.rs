mod api;
mod db;
mod jmap;

use std::sync::{Arc, Mutex};

use axum::routing::get;
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let jmap_url = std::env::var("JMAP_URL").ok();
    let jmap_username = std::env::var("JMAP_USERNAME").ok();
    let jmap_password = std::env::var("JMAP_PASSWORD").ok();

    std::fs::create_dir_all("data").ok();
    let conn = db::init_db("data/mail.sqlite");

    if let (Some(url), Some(user), Some(pass)) = (jmap_url, jmap_username, jmap_password) {
        println!("Connecting to JMAP server at {url}...");
        match jmap::connect(&url, &user, &pass).await {
            Ok(client) => {
                println!("Connected! Syncing...");
                if let Err(e) = jmap::sync_mailboxes(&client, &conn).await {
                    eprintln!("Failed to sync mailboxes: {e}");
                }
                if let Err(e) = jmap::sync_emails(&client, &conn).await {
                    eprintln!("Failed to sync emails: {e}");
                }
                println!("Sync complete.");
            }
            Err(e) => {
                eprintln!("Failed to connect to JMAP: {e}");
                eprintln!("Falling back to mock data.");
                db::import_mock_data(&conn);
            }
        }
    } else {
        println!("No JMAP credentials. Using mock data.");
        db::import_mock_data(&conn);
    }

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
