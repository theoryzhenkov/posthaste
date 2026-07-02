use super::*;

fn base_message() -> CacheMessageSignals {
    CacheMessageSignals {
        age_days: 2.0,
        in_inbox: true,
        unread: true,
        flagged: false,
        thread_activity: 0.0,
        sender_affinity: 0.0,
        local_behavior: 0.0,
        search: None,
    }
}

mod admission;
mod scoring;
