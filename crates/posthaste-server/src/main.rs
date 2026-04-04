use posthaste_server::{start_server, ServerConfig};

#[tokio::main]
async fn main() {
    let handle = start_server(ServerConfig::default()).await;
    handle
        .join_handle
        .await
        .expect("daemon server task panicked");
}
