use super::helpers::visible_rank_direct_boost;
use super::*;

impl MailService {
    pub fn record_cache_search_visibility(
        &self,
        page: &MessagePage,
        total_messages: u64,
        result_count: u64,
    ) -> Result<Vec<AccountId>, ServiceError> {
        if page.items.is_empty() {
            return Ok(Vec::new());
        }
        let total_messages = total_messages
            .max(result_count)
            .max(page.items.len() as u64);
        let result_count = result_count.max(page.items.len() as u64);
        let updates = page
            .items
            .iter()
            .enumerate()
            .map(|(rank, message)| CacheSignalUpdate {
                account_id: message.source_id.to_string(),
                message_id: message.id.to_string(),
                reason: "search-visible".to_string(),
                search: Some(crate::CacheSearchSignals {
                    total_messages,
                    result_count,
                    result_rank: rank as u64,
                }),
                thread_activity: None,
                sender_affinity: None,
                local_behavior: None,
                direct_user_boost: Some(visible_rank_direct_boost(rank as u64)),
                pinned: None,
            })
            .collect::<Vec<_>>();
        self.cache_store.record_cache_signal_updates(&updates)?;
        let account_ids = updates
            .iter()
            .map(|update| AccountId::from(update.account_id.as_str()))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        ph_debug!(
            events::CACHE_SEARCH_VISIBILITY_RECORDED,
            message_count = updates.len(),
            account_count = account_ids.len(),
            total_messages,
            result_count,
            "cache search visibility signals recorded"
        );
        Ok(account_ids)
    }
}
