mod api;
mod db;
mod jmap;
mod sanitize;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub struct AppState {
    pub db: Mutex<rusqlite::Connection>,
    pub jmap_client: Option<jmap_client::client::Client>,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let jmap_url = std::env::var("JMAP_URL").ok();
    let jmap_username = std::env::var("JMAP_USERNAME").ok();
    let jmap_password = std::env::var("JMAP_PASSWORD").ok();

    std::fs::create_dir_all("data").expect("failed to create data directory");
    let conn = db::init_db("data/mail.sqlite").expect("failed to initialize database");

    let mut jmap_client = None;

    if let (Some(url), Some(user), Some(pass)) = (jmap_url, jmap_username, jmap_password) {
        println!("Connecting to JMAP server at {url}...");
        match jmap::connect(&url, &user, &pass).await {
            Ok(client) => {
                let has_state = db::get_sync_state(&conn, "email")
                    .ok()
                    .flatten()
                    .is_some();
                let mode = if has_state { "incremental" } else { "full, first run" };
                println!("Connected! Syncing ({mode})...");

                if let Err(e) = jmap::sync_mailboxes(&client, &conn).await {
                    eprintln!("Failed to sync mailboxes: {e}");
                }
                if let Err(e) = jmap::sync_emails(&client, &conn).await {
                    eprintln!("Failed to sync emails: {e}");
                }
                println!("Sync complete.");
                jmap_client = Some(client);
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
        jmap_client,
    });

    // Background sync: JMAP delta sync every 60s
    if state.jmap_client.is_some() {
        let sync_state = state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            interval.tick().await; // skip the first immediate tick
            loop {
                interval.tick().await;
                let client = sync_state.jmap_client.as_ref().unwrap();

                // --- Mailbox sync ---
                let mb_state = {
                    let conn = sync_state.db.lock().unwrap();
                    db::get_sync_state(&conn, "mailbox").ok().flatten()
                };
                match jmap::fetch_mailbox_sync(client, mb_state.as_deref()).await {
                    Ok(result) => {
                        let has_changes = result.created_count > 0
                            || result.updated_count > 0
                            || result.destroyed_count > 0;
                        if has_changes {
                            eprintln!(
                                "[sync] Mailbox: {} created, {} updated, {} destroyed",
                                result.created_count, result.updated_count, result.destroyed_count,
                            );
                        }
                        let conn = sync_state.db.lock().unwrap();
                        if let Err(e) = jmap::apply_mailbox_sync(&conn, &result) {
                            eprintln!("[sync] mailbox write error: {e}");
                        }
                    }
                    Err(e) => eprintln!("[sync] mailbox fetch error: {e}"),
                }

                // --- Email sync ---
                let em_state = {
                    let conn = sync_state.db.lock().unwrap();
                    db::get_sync_state(&conn, "email").ok().flatten()
                };
                match jmap::fetch_email_sync(client, em_state.as_deref()).await {
                    Ok(result) => {
                        let has_changes = result.created_count > 0
                            || result.updated_count > 0
                            || result.destroyed_count > 0;
                        if has_changes {
                            eprintln!(
                                "[sync] Email: {} created, {} updated, {} destroyed",
                                result.created_count, result.updated_count, result.destroyed_count,
                            );
                        }
                        let conn = sync_state.db.lock().unwrap();
                        if let Err(e) = jmap::apply_email_sync(&conn, &result) {
                            eprintln!("[sync] email write error: {e}");
                        }
                    }
                    Err(e) => eprintln!("[sync] email fetch error: {e}"),
                }
            }
        });
    }

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
        .route("/api/emails/{id}/body", get(api::get_email_body))
        .route("/api/emails/{id}/actions", post(api::post_email_action))
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
