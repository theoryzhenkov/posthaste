//! JMAP mail engine -- gateway implementations, sync loop, push transports, and compose helpers.
//!
//! @spec spec/L1-jmap
//! @spec spec/L1-sync
//! @spec spec/L2-transport

mod compose;
mod conversions;
mod live;
mod mock;
mod push_sse;
mod push_ws;
mod sync;
mod ws_connection;

pub use live::{connect_jmap_client, LiveJmapGateway};
pub use mock::MockJmapGateway;
pub use push_sse::SsePushTransport;
pub use push_ws::WsPushTransport;
pub use ws_connection::SharedWsConnection;

/// JMAP data types monitored for push notifications and sync triggers.
///
/// @spec spec/L1-jmap#push
const WATCHED_DATA_TYPES: [jmap_client::DataType; 4] = [
    jmap_client::DataType::Email,
    jmap_client::DataType::Mailbox,
    jmap_client::DataType::EmailDelivery,
    jmap_client::DataType::EmailSubmission,
];
