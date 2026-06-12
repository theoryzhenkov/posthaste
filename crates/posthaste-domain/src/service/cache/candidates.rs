use super::helpers::{body_fetch_bytes, body_fetch_unit, estimated_body_bytes, message_age_days};
use super::*;

impl MailService {
    pub(crate) fn upsert_body_cache_candidates(
        &self,
        account_id: &AccountId,
        account: &AccountSettings,
        policy: &CachePolicy,
        messages: &[MessageRecord],
    ) -> Result<(), ServiceError> {
        if !policy.cache_bodies || messages.is_empty() {
            ph_debug!(
                events::CACHE_BODY_CANDIDATE_GENERATION_SKIPPED,
                account_id = %account_id,
                message_count = messages.len(),
                cache_bodies = policy.cache_bodies,
                "cache candidate generation skipped"
            );
            return Ok(());
        }

        let inbox_mailbox_ids = self
            .mailbox_reader
            .list_mailboxes(account_id)?
            .into_iter()
            .filter(|mailbox| mailbox.role.as_deref() == Some("inbox"))
            .map(|mailbox| mailbox.id)
            .collect::<HashSet<_>>();
        let fetch_unit = body_fetch_unit(account);
        let candidates = messages
            .iter()
            .filter(|message| message.body_html.is_none() && message.body_text.is_none())
            .map(|message| {
                let value_bytes = estimated_body_bytes(message);
                let fetch_bytes = body_fetch_bytes(account, message);
                let signals = CacheCandidateSignals {
                    message: CacheMessageSignals {
                        age_days: message_age_days(&message.received_at),
                        in_inbox: message
                            .mailbox_ids
                            .iter()
                            .any(|mailbox_id| inbox_mailbox_ids.contains(mailbox_id)),
                        unread: !message.keywords.iter().any(|keyword| keyword == "$seen"),
                        flagged: message.keywords.iter().any(|keyword| keyword == "$flagged"),
                        thread_activity: 0.0,
                        sender_affinity: 0.0,
                        local_behavior: 0.0,
                        search: None,
                    },
                    layer: CacheLayer::Body,
                    fetch_unit,
                    value_bytes,
                    fetch_bytes,
                    inline_attachment: false,
                    opened_attachment: false,
                    direct_user_boost: 0.0,
                    pinned: false,
                };
                let score = score_cache_candidate(&signals);
                ph_trace!(
                    events::CACHE_BODY_CANDIDATE_SCORED,
                    account_id = %account_id,
                    message_id = %message.id,
                    layer = CacheLayer::Body.as_str(),
                    fetch_unit = fetch_unit.as_str(),
                    value_bytes,
                    fetch_bytes,
                    utility = score.utility,
                    size_cost = score.size_cost,
                    priority = score.priority,
                    age_days = signals.message.age_days,
                    in_inbox = signals.message.in_inbox,
                    unread = signals.message.unread,
                    flagged = signals.message.flagged,
                    "cache body candidate scored"
                );
                CacheCandidate {
                    account_id: account_id.to_string(),
                    message_id: message.id.to_string(),
                    layer: CacheLayer::Body,
                    object_id: None,
                    fetch_unit,
                    value_bytes,
                    fetch_bytes,
                    priority: score.priority,
                    reason: match fetch_unit {
                        CacheFetchUnit::BodyOnly => "body".to_string(),
                        CacheFetchUnit::RawMessage => "body-via-raw-message".to_string(),
                        CacheFetchUnit::AttachmentBlob => "body".to_string(),
                    },
                }
            })
            .collect::<Vec<_>>();
        let total_fetch_bytes = candidates
            .iter()
            .map(|candidate| candidate.fetch_bytes)
            .sum::<u64>();
        let total_value_bytes = candidates
            .iter()
            .map(|candidate| candidate.value_bytes)
            .sum::<u64>();
        ph_debug!(
            events::CACHE_BODY_CANDIDATES_SCORED,
            account_id = %account_id,
            driver = ?account.driver,
            fetch_unit = fetch_unit.as_str(),
            synced_message_count = messages.len(),
            candidate_count = candidates.len(),
            total_value_bytes,
            total_fetch_bytes,
            "cache body candidates scored"
        );
        self.cache_store.upsert_cache_candidates(&candidates)?;
        ph_debug!(
            events::CACHE_BODY_CANDIDATES_UPSERTED,
            account_id = %account_id,
            candidate_count = candidates.len(),
            "cache body candidates upserted"
        );
        Ok(())
    }
}
