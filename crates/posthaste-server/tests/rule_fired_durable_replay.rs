//! RFC-L2-scripting S3 (AS tap mount) — the durability half of the resolution.
//!
//! Investigation finding: the bundled server does not need a SECOND
//! `/v1/events` mount for the authority server's own bus — the runtime shares
//! the AS's `event_sender` and `event_log` in-process (D52's one-tap
//! discipline), so the existing S1/S2 tap already carries AS-origin facts.
//! What it did NOT do durably was the rule engine's meta-facts: `rule.fired`
//! was built by hand with `seq: 0` and sent straight to the live broadcast,
//! bypassing `event_log` entirely — visible to a live subscriber, silently
//! GONE (no fact, no gap frame) to anyone who reconnects. `AuthorityServerFactLog`
//! (`posthaste-authority-server/src/fact_log.rs`) closes that: the rule engine
//! now appends through the same durable, seq-assigning `FactLog::append` the
//! tap replays from.
//!
//! This proves it on the real wire path (the bundled server exactly as
//! `posthaste_server::start_server` composes it, same as
//! `rules_webhook_e2e.rs`): an `emit` rule (RFC §7.19: fires ONLY the
//! `rule.fired` fact) matches a tagged message, and a FRESH
//! `GET /v1/events?afterSeq=0` (a reconnect, not the live stream the rule
//! fired on) still replays `rule.fired` from durable history, carrying a real
//! (nonzero) `seq`.
//!
//! Split into its own binary/process (see `rule_fired_support`) alongside the
//! gap-frame half in `rule_fired_gap_frame.rs`.

mod rule_fired_support;

use rule_fired_support::{seq_of_topic_frame, Harness, EMIT_RULE_TOML};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rule_fired_is_durably_replayed_after_a_reconnect() {
    let harness = Harness::start("rule-fired-durable", EMIT_RULE_TOML).await;

    let message_id = harness.seed_account("acct-b").await;
    harness.tag_instruct("acct-b", &message_id).await;

    // A FRESH subscription from the very start (a "reconnect", not the live
    // stream the rule fired on) must still carry `rule.fired` from the durable
    // backlog. Before the fix this fact never reached `event_log` at all (it
    // was sent with `seq: 0` straight to the live broadcast), so a reconnect
    // like this one would silently never see it again.
    let sse = harness
        .events_containing(0, &["\"topic\":\"rule.fired\""])
        .await;
    let seq = seq_of_topic_frame(&sse, "rule.fired");
    assert!(
        seq > 0,
        "a durably-replayed rule.fired must carry its real assigned seq, not the old placeholder 0: {sse}"
    );
    assert!(
        !sse.contains("event: gap"),
        "nothing has been truncated yet — a plain replay must not gap: {sse}"
    );

    harness.shutdown().await;
}
