use serde::{Deserialize, Serialize};

/// Backend operational tuning for daemon runtime internals.
///
/// These values mirror current hard-coded backend defaults. They are exposed in
/// config so call sites can be wired incrementally without changing behavior.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct DaemonRuntimeTuning {
    pub supervisor: SupervisorRuntimeTuning,
    pub oauth: OAuthRuntimeTuning,
    pub push: PushRuntimeTuning,
    pub sync: SyncRuntimeTuning,
    pub store: StoreRuntimeTuning,
}

impl DaemonRuntimeTuning {
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct SupervisorRuntimeTuning {
    pub automation_backfill_batch_size: usize,
    pub automation_backfill_initial_delay_seconds: u64,
    pub automation_backfill_interval_seconds: u64,
    pub cache_worker_initial_delay_seconds: u64,
    pub cache_worker_interval_seconds: u64,
    pub cache_stale_rescore_after_seconds: u64,
    pub cache_background_pressure: f64,
    pub cache_interactive_pressure: f64,
    pub command_channel_buffer_size: usize,
    pub event_broadcast_buffer_size: usize,
}

impl Default for SupervisorRuntimeTuning {
    fn default() -> Self {
        Self {
            automation_backfill_batch_size: 10,
            automation_backfill_initial_delay_seconds: 10,
            automation_backfill_interval_seconds: 15,
            cache_worker_initial_delay_seconds: 5,
            cache_worker_interval_seconds: 2,
            cache_stale_rescore_after_seconds: 6 * 60 * 60,
            cache_background_pressure: 0.0,
            cache_interactive_pressure: 1.0,
            command_channel_buffer_size: 32,
            event_broadcast_buffer_size: 512,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct OAuthRuntimeTuning {
    pub refresh_skew_seconds: i64,
    pub jwks_default_cache_seconds: i64,
    pub jwks_max_cache_seconds: i64,
}

impl Default for OAuthRuntimeTuning {
    fn default() -> Self {
        Self {
            refresh_skew_seconds: 300,
            jwks_default_cache_seconds: 3_600,
            jwks_max_cache_seconds: 86_400,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct PushRuntimeTuning {
    pub jmap_sse_ping_seconds: u64,
    pub api_sse_keep_alive_seconds: u64,
    pub resilient_initial_retry_delay_seconds: u64,
    pub resilient_max_retry_delay_seconds: u64,
    pub resilient_fallback_threshold: u32,
    pub imap_idle_reconnect_delay_seconds: u64,
}

impl Default for PushRuntimeTuning {
    fn default() -> Self {
        Self {
            jmap_sse_ping_seconds: 60,
            api_sse_keep_alive_seconds: 15,
            resilient_initial_retry_delay_seconds: 5,
            resilient_max_retry_delay_seconds: 120,
            resilient_fallback_threshold: 3,
            imap_idle_reconnect_delay_seconds: 30,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct SyncRuntimeTuning {
    pub jmap_mailbox_changes_max_changes: usize,
    pub jmap_email_changes_max_changes: usize,
    pub jmap_email_get_chunk_size: usize,
    pub jmap_full_email_get_chunk_size: usize,
    pub imap_uid_fetch_chunk_size: usize,
    pub api_default_page_size: usize,
    pub api_max_page_size: usize,
    pub store_message_value_chunk_size: usize,
}

impl Default for SyncRuntimeTuning {
    fn default() -> Self {
        Self {
            jmap_mailbox_changes_max_changes: 500,
            jmap_email_changes_max_changes: 500,
            jmap_email_get_chunk_size: 100,
            jmap_full_email_get_chunk_size: 100,
            imap_uid_fetch_chunk_size: 128,
            api_default_page_size: 100,
            api_max_page_size: 250,
            store_message_value_chunk_size: 400,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct StoreRuntimeTuning {
    pub sqlite_busy_timeout_seconds: u64,
    pub sender_address_cache_cap: usize,
}

impl Default for StoreRuntimeTuning {
    fn default() -> Self {
        Self {
            sqlite_busy_timeout_seconds: 5,
            sender_address_cache_cap: 40,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_tuning_defaults_match_current_backend_constants() {
        let tuning = DaemonRuntimeTuning::default();

        assert_eq!(tuning.supervisor.automation_backfill_batch_size, 10);
        assert_eq!(tuning.supervisor.cache_stale_rescore_after_seconds, 21_600);
        assert_eq!(tuning.supervisor.command_channel_buffer_size, 32);
        assert_eq!(tuning.supervisor.event_broadcast_buffer_size, 512);
        assert_eq!(tuning.oauth.refresh_skew_seconds, 300);
        assert_eq!(tuning.oauth.jwks_default_cache_seconds, 3_600);
        assert_eq!(tuning.push.jmap_sse_ping_seconds, 60);
        assert_eq!(tuning.push.api_sse_keep_alive_seconds, 15);
        assert_eq!(tuning.push.resilient_initial_retry_delay_seconds, 5);
        assert_eq!(tuning.push.resilient_max_retry_delay_seconds, 120);
        assert_eq!(tuning.sync.jmap_mailbox_changes_max_changes, 500);
        assert_eq!(tuning.sync.jmap_email_get_chunk_size, 100);
        assert_eq!(tuning.sync.imap_uid_fetch_chunk_size, 128);
        assert_eq!(tuning.sync.store_message_value_chunk_size, 400);
        assert_eq!(tuning.store.sqlite_busy_timeout_seconds, 5);
        assert_eq!(tuning.store.sender_address_cache_cap, 40);
    }

    #[test]
    fn runtime_tuning_partial_toml_uses_defaults_for_missing_values() {
        let parsed: DaemonRuntimeTuning = toml::from_str(
            r#"
            [supervisor]
            command_channel_buffer_size = 64

            [push]
            resilient_fallback_threshold = 5
            "#,
        )
        .unwrap();

        assert_eq!(parsed.supervisor.command_channel_buffer_size, 64);
        assert_eq!(parsed.supervisor.automation_backfill_batch_size, 10);
        assert_eq!(parsed.push.resilient_fallback_threshold, 5);
        assert_eq!(parsed.push.resilient_initial_retry_delay_seconds, 5);
        assert_eq!(parsed.store.sqlite_busy_timeout_seconds, 5);
    }
}
