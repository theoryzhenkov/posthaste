use super::*;

#[test]
fn cache_resource_governor_caps_background_fetch_lease() {
    let now = Instant::now();
    let mut governor = CacheResourceGovernor::new(now, CacheResourcePolicy::default());

    let lease = governor.grant(now, 0.0);

    assert!(lease.rescore_limit > 0);
    assert!(lease.fetch.request_limit <= 2);
    assert!(lease.fetch.byte_limit <= 4 * 1024 * 1024);
    assert!(!lease.in_backoff);
}

#[test]
fn cache_resource_governor_grants_interactive_burst() {
    let now = Instant::now();
    let mut governor = CacheResourceGovernor::new(now, CacheResourcePolicy::default());

    let lease = governor.grant(now, 1.0);

    assert!(lease.stale_rescore_limit == 0);
    assert!(lease.rescore_limit >= 200);
    assert!(lease.fetch.request_limit >= 1);
    assert!(lease.fetch.byte_limit >= 4 * 1024 * 1024);
}

#[test]
fn cache_resource_governor_backs_off_after_failed_fetches() {
    let now = Instant::now();
    let mut governor = CacheResourceGovernor::new(now, CacheResourcePolicy::default());
    let lease = governor.grant(now, 1.0);

    governor.record_feedback(
        now,
        &lease,
        CacheMaintenanceFeedback {
            fetch_attempted: 1,
            fetch_attempted_bytes: 32 * 1024,
            fetch_failed: 1,
            ..Default::default()
        },
    );
    let backoff_lease = governor.grant(now + Duration::from_secs(1), 1.0);

    assert!(backoff_lease.in_backoff);
    assert_eq!(backoff_lease.fetch.request_limit, 0);
    assert!(governor.network_rate_multiplier() < 1.0);
}

#[test]
fn cache_resource_governor_does_not_network_backoff_for_local_errors() {
    let now = Instant::now();
    let mut governor = CacheResourceGovernor::new(now, CacheResourcePolicy::default());
    let lease = governor.grant(now, 1.0);

    governor.record_feedback(
        now,
        &lease,
        CacheMaintenanceFeedback {
            had_error: true,
            ..Default::default()
        },
    );
    let next_lease = governor.grant(now + Duration::from_secs(1), 1.0);

    assert!(!next_lease.in_backoff);
    assert_eq!(governor.network_rate_multiplier(), 1.0);
}

#[test]
fn cache_resource_governor_refunds_unused_lease() {
    let now = Instant::now();
    let mut governor = CacheResourceGovernor::new(now, CacheResourcePolicy::default());
    let lease = governor.grant(now, 0.0);

    governor.record_feedback(now, &lease, CacheMaintenanceFeedback::default());
    let next_lease = governor.grant(now, 0.0);

    assert_eq!(next_lease.fetch.request_limit, lease.fetch.request_limit);
    assert_eq!(next_lease.fetch.byte_limit, lease.fetch.byte_limit);
}

// spec: docs/L1-sync#cache-priority-size-aware
#[test]
fn body_priority_uses_fetch_unit_cost_not_body_value_size() {
    let jmap_score = score_cache_candidate(&CacheCandidateSignals {
        message: base_message(),
        layer: CacheLayer::Body,
        fetch_unit: CacheFetchUnit::BodyOnly,
        value_bytes: 64 * 1024,
        fetch_bytes: 64 * 1024,
        inline_attachment: false,
        opened_attachment: false,
        direct_user_boost: 0.0,
        pinned: false,
    });
    let imap_score = score_cache_candidate(&CacheCandidateSignals {
        message: base_message(),
        layer: CacheLayer::Body,
        fetch_unit: CacheFetchUnit::RawMessage,
        value_bytes: 64 * 1024,
        fetch_bytes: 12 * 1024 * 1024,
        inline_attachment: false,
        opened_attachment: false,
        direct_user_boost: 0.0,
        pinned: false,
    });

    assert!(jmap_score.priority > imap_score.priority);
}

// spec: docs/L1-sync#cache-admission-hard-cap
#[test]
fn interactive_pressure_raises_target_between_soft_and_hard_caps() {
    let limits = CacheBudget {
        used_bytes: 900,
        soft_cap_bytes: 1_000,
        hard_cap_bytes: 2_000,
        interactive_pressure: 0.75,
    };

    assert_eq!(limits.effective_target_bytes(), 1_750);
}

// spec: docs/L1-sync#cache-admission-hard-cap
#[test]
fn admission_allows_soft_cap_burst_but_never_crosses_hard_cap() {
    let limits = CacheBudget {
        used_bytes: 1_600,
        soft_cap_bytes: 1_000,
        hard_cap_bytes: 2_000,
        interactive_pressure: 0.75,
    };

    assert_eq!(
        decide_cache_admission(100, 2.0, Some(1.0), &limits),
        CacheAdmission::AdmitWithinTarget
    );
    assert_eq!(
        decide_cache_admission(500, 10.0, Some(1.0), &limits),
        CacheAdmission::RejectOverHardCap
    );
}

// spec: docs/L1-sync#cache-priority-size-aware
#[test]
fn admission_requires_beating_evictable_priority_when_over_target() {
    let limits = CacheBudget {
        used_bytes: 1_900,
        soft_cap_bytes: 1_000,
        hard_cap_bytes: 3_000,
        interactive_pressure: 0.25,
    };

    assert_eq!(
        decide_cache_admission(100, 0.5, Some(1.0), &limits),
        CacheAdmission::RejectLowerPriority
    );
    assert_eq!(
        decide_cache_admission(100, 1.5, Some(1.0), &limits),
        CacheAdmission::AdmitByReplacingLowerPriority
    );
}
