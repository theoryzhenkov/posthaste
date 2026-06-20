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
        r"
        [supervisor]
        command_channel_buffer_size = 64

        [push]
        resilient_fallback_threshold = 5
        ",
    )
    .unwrap();

    assert_eq!(parsed.supervisor.command_channel_buffer_size, 64);
    assert_eq!(parsed.supervisor.automation_backfill_batch_size, 10);
    assert_eq!(parsed.push.resilient_fallback_threshold, 5);
    assert_eq!(parsed.push.resilient_initial_retry_delay_seconds, 5);
    assert_eq!(parsed.store.sqlite_busy_timeout_seconds, 5);
}
