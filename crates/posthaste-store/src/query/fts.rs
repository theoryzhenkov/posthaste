use super::*;

impl DatabaseStore {
    /// Prototype full-text search over indexed message header fields (subject,
    /// from name/email, preview) via the `message_fts` FTS5 index.
    ///
    /// `query` is an FTS5 MATCH expression (e.g. `invoice` or `subject:invoice`).
    /// Returns summaries newest-first, capped at `limit`.
    pub fn fts_search_messages(
        &self,
        account_id: &AccountId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare_cached(
                "SELECT m.id, m.account_id, COALESCE(a.name, m.account_id), m.thread_id, m.conversation_id, m.subject,
                        m.from_name, m.from_email, m.to_json, m.preview, m.received_at,
                        m.has_attachment, m.is_read, m.is_flagged
                 FROM message_fts
                 JOIN message m ON m.rowid = message_fts.rowid
                 LEFT JOIN source_projection a ON a.source_id = m.account_id
                 WHERE m.account_id = ?1 AND message_fts MATCH ?2
                 ORDER BY m.received_at DESC
                 LIMIT ?3",
            )
            .map_err(sql_to_store_error)?;
        let rows = load_message_summary_rows(
            &mut statement,
            params![account_id.as_str(), query, limit as i64],
        )?;
        hydrate_message_summaries(&connection, rows)
    }
}
