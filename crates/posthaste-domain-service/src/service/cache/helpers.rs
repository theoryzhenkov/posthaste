use super::*;

pub(super) fn message_age_days(received_at: &str) -> f64 {
    let Ok(received_at) =
        time::OffsetDateTime::parse(received_at, &time::format_description::well_known::Rfc3339)
    else {
        return 365.0;
    };
    let now = time::OffsetDateTime::now_utc();
    let seconds = (now - received_at).whole_seconds().max(0) as f64;
    seconds / 86_400.0
}

pub(super) fn nonnegative_message_size(size: i64) -> u64 {
    u64::try_from(size.max(0)).unwrap_or(0)
}

pub(super) fn estimated_body_bytes(message: &MessageRecord) -> u64 {
    estimated_body_bytes_from_metadata(message.size, message.has_attachment)
}

pub(super) fn estimated_body_bytes_from_metadata(size: i64, has_attachment: bool) -> u64 {
    let metadata_size = nonnegative_message_size(size);
    if metadata_size == 0 {
        return 64 * 1024;
    }
    if has_attachment {
        metadata_size.clamp(16 * 1024, 256 * 1024)
    } else {
        metadata_size.max(4 * 1024)
    }
}

pub(super) fn body_fetch_unit(account: &AccountSettings) -> CacheFetchUnit {
    match account.driver {
        AccountDriver::ImapSmtp => CacheFetchUnit::RawMessage,
        AccountDriver::Jmap | AccountDriver::Mock => CacheFetchUnit::BodyOnly,
    }
}

pub(super) fn body_fetch_bytes(account: &AccountSettings, message: &MessageRecord) -> u64 {
    body_fetch_bytes_from_metadata(account, message.size, message.has_attachment)
}

pub(super) fn body_fetch_bytes_from_metadata(
    account: &AccountSettings,
    size: i64,
    has_attachment: bool,
) -> u64 {
    match body_fetch_unit(account) {
        CacheFetchUnit::RawMessage => nonnegative_message_size(size).max(4 * 1024),
        CacheFetchUnit::BodyOnly => estimated_body_bytes_from_metadata(size, has_attachment),
        CacheFetchUnit::AttachmentBlob => unreachable!("body cache never fetches attachment blobs"),
    }
}

pub(super) fn visible_rank_direct_boost(rank: u64) -> f64 {
    0.8 / ((rank + 1) as f64).sqrt()
}

pub(super) fn rescore_candidate_signals(
    candidate: &CacheRescoreCandidate,
    fetch_unit: CacheFetchUnit,
    value_bytes: u64,
    fetch_bytes: u64,
) -> CacheCandidateSignals {
    CacheCandidateSignals {
        message: CacheMessageSignals {
            age_days: message_age_days(&candidate.received_at),
            in_inbox: candidate.in_inbox,
            unread: candidate.unread,
            flagged: candidate.flagged,
            thread_activity: candidate.thread_activity,
            sender_affinity: candidate.sender_affinity,
            local_behavior: candidate.local_behavior,
            search: candidate.search.clone(),
        },
        layer: candidate.layer,
        fetch_unit,
        value_bytes,
        fetch_bytes,
        inline_attachment: false,
        opened_attachment: false,
        direct_user_boost: candidate.direct_user_boost,
        pinned: candidate.pinned,
    }
}
