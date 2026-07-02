use super::*;

// spec: docs/L1-sync#cache-priority-size-aware
#[test]
fn recent_unread_body_scores_above_old_unread_body() {
    let mut old = base_message();
    old.age_days = 365.0;

    let recent_score = score_cache_candidate(&CacheCandidateSignals {
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
    let old_score = score_cache_candidate(&CacheCandidateSignals {
        message: old,
        layer: CacheLayer::Body,
        fetch_unit: CacheFetchUnit::BodyOnly,
        value_bytes: 64 * 1024,
        fetch_bytes: 64 * 1024,
        inline_attachment: false,
        opened_attachment: false,
        direct_user_boost: 0.0,
        pinned: false,
    });

    assert!(recent_score.priority > old_score.priority);
}

// spec: docs/L1-sync#cache-priority-size-aware
#[test]
fn high_value_large_attachment_can_beat_low_value_small_attachment() {
    let mut high_value = base_message();
    high_value.flagged = true;
    high_value.thread_activity = 5.0;
    high_value.sender_affinity = 5.0;

    let mut low_value = base_message();
    low_value.age_days = 180.0;
    low_value.in_inbox = false;
    low_value.unread = false;

    let large_score = score_cache_candidate(&CacheCandidateSignals {
        message: high_value,
        layer: CacheLayer::AttachmentBlob,
        fetch_unit: CacheFetchUnit::AttachmentBlob,
        value_bytes: 20 * 1024 * 1024,
        fetch_bytes: 20 * 1024 * 1024,
        inline_attachment: false,
        opened_attachment: true,
        direct_user_boost: 0.0,
        pinned: false,
    });
    let small_score = score_cache_candidate(&CacheCandidateSignals {
        message: low_value,
        layer: CacheLayer::AttachmentBlob,
        fetch_unit: CacheFetchUnit::AttachmentBlob,
        value_bytes: 1024 * 1024,
        fetch_bytes: 1024 * 1024,
        inline_attachment: false,
        opened_attachment: false,
        direct_user_boost: 0.0,
        pinned: false,
    });

    assert!(large_score.priority > small_score.priority);
}

// spec: docs/L1-sync#cache-priority-size-aware
#[test]
fn tight_visible_search_result_boosts_priority() {
    let mut searched = base_message();
    searched.search = Some(CacheSearchSignals {
        total_messages: 100_000,
        result_count: 12,
        result_rank: 0,
    });

    let searched_score = score_cache_candidate(&CacheCandidateSignals {
        message: searched,
        layer: CacheLayer::Body,
        fetch_unit: CacheFetchUnit::BodyOnly,
        value_bytes: 64 * 1024,
        fetch_bytes: 64 * 1024,
        inline_attachment: false,
        opened_attachment: false,
        direct_user_boost: 0.0,
        pinned: false,
    });
    let baseline_score = score_cache_candidate(&CacheCandidateSignals {
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

    assert!(searched_score.priority > baseline_score.priority);
}

#[test]
fn local_signal_rescore_priority_beats_background_work() {
    let priority = cache_signal_rescore_priority(&CacheSignalUpdate {
        account_id: "primary".to_string(),
        message_id: "message-1".to_string(),
        reason: "search-visible".to_string(),
        search: Some(CacheSearchSignals {
            total_messages: 1_000,
            result_count: 10,
            result_rank: 0,
        }),
        thread_activity: None,
        sender_affinity: None,
        local_behavior: None,
        direct_user_boost: Some(0.8),
        pinned: None,
    });

    assert!(priority > LOCAL_SIGNAL_RESCORE_BASE);
}
