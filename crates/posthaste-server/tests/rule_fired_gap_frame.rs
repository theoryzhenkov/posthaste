//! RFC-L2-scripting S3 (AS tap mount) — the gap-frame half of the resolution.
//!
//! Companion to `rule_fired_durable_replay.rs` (see its header for the full
//! finding). This proves the other half of D52/§3's durability contract for an
//! AS-origin fact: a cursor that falls before the log's truncation point never
//! gets a silent drop — it gets the explicit **gap frame** — and the retained
//! tail served right after it still carries the durable `rule.fired` fact.
//!
//! Split into its own binary/process (see `rule_fired_support`): the bundled
//! server's `start_server` initializes a process-global tracing subscriber
//! exactly once, so two tests that each start a server cannot share a binary.

mod rule_fired_support;

use rule_fired_support::{Harness, EMIT_RULE_TOML};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rule_fired_survives_the_gap_frame_as_a_retained_tail() {
    let harness = Harness::start("rule-fired-gap", EMIT_RULE_TOML).await;

    // acct-a is created and synced FIRST, so it owns the event_log's oldest
    // rows; acct-b (and its rule.fired) come strictly later and get higher
    // seqs. Deleting acct-a purges only ITS rows (source.rs's per-account
    // purge), advancing the global truncation point past them while acct-b's
    // facts — including rule.fired — survive.
    harness.seed_account("acct-a").await;
    let message_id = harness.seed_account("acct-b").await;
    harness.tag_instruct("acct-b", &message_id).await;

    // Confirm rule.fired is reachable from seq 0 BEFORE the deletion (the
    // cursor this test re-uses once it is no longer serviceable).
    harness
        .events_containing(0, &["\"topic\":\"rule.fired\""])
        .await;

    harness.delete_account("acct-a").await;

    // The SAME cursor (0) now falls before the new truncation point: the
    // reconnect opens with the explicit gap frame, THEN still serves the
    // retained tail — which must still include rule.fired.
    let sse = harness
        .events_containing(
            0,
            &["event: gap", "\"kind\":\"reset\"", "\"topic\":\"rule.fired\""],
        )
        .await;

    // The gap frame must appear before the retained rule.fired frame — never
    // interleaved after data the consumer would misread as a clean
    // continuation.
    let gap_at = sse.find("event: gap").expect("gap frame present");
    let fact_at = sse
        .find("\"topic\":\"rule.fired\"")
        .expect("retained rule.fired present");
    assert!(
        gap_at < fact_at,
        "the gap frame must precede the retained tail it explains: {sse}"
    );

    harness.shutdown().await;
}
