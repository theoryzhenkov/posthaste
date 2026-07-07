use super::*;

/// FTS5 `bm25()` column weights for `message_fts`, in column order
/// `(subject, from_name, from_email, preview, body)`. Header hits outrank
/// body-only hits: a term in the subject weighs 8x a term in the body, sender
/// name/email 4x, and the preview snippet 2x — so a message that matches only
/// deep in its cached body sorts below one whose subject or sender matches.
const FTS_BM25_WEIGHTS: &str = "8.0, 4.0, 4.0, 2.0, 1.0";

impl DatabaseStore {
    /// Full-text search over indexed message fields — subject, from
    /// name/email, preview, and the cached body text — via the `message_fts`
    /// FTS5 index. Bodies are indexed the moment the body cache stores them
    /// (see the `message_body_fts_*` triggers); a message whose body has not
    /// been cached yet matches on header fields only.
    ///
    /// `query` is an FTS5 MATCH expression (e.g. `invoice` or `body:invoice`).
    /// Returns summaries ranked by weighted bm25 relevance
    /// ([`FTS_BM25_WEIGHTS`]: subject/sender hits above body-only hits),
    /// newest-first among equal ranks, capped at `limit`.
    pub fn fts_search_messages(
        &self,
        account_id: &AccountId,
        query: &str,
        limit: usize,
    ) -> Result<Vec<MessageSummary>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare_cached(&format!(
                "SELECT m.id, m.account_id, COALESCE(a.name, m.account_id), m.thread_id, m.conversation_id, m.subject,
                        m.from_name, m.from_email, m.to_json, m.preview, m.received_at,
                        m.has_attachment, m.is_read, m.is_flagged
                 FROM message_fts
                 JOIN message m ON m.rowid = message_fts.rowid
                 LEFT JOIN source_projection a ON a.source_id = m.account_id
                 WHERE m.account_id = ?1 AND message_fts MATCH ?2
                 ORDER BY bm25(message_fts, {FTS_BM25_WEIGHTS}), m.received_at DESC
                 LIMIT ?3"
            ))
            .map_err(sql_to_store_error)?;
        let rows = load_message_summary_rows(
            &mut statement,
            params![account_id.as_str(), query, limit as i64],
        )?;
        hydrate_message_summaries(&connection, rows)
    }

    /// One-time repopulation of the `message_fts` index after the
    /// body-indexing migration (`migrate_legacy_message_fts`) dropped the old
    /// header-only table: when messages exist but the index is empty, issue
    /// the FTS5 external-content `rebuild`, which re-reads every row of the
    /// `message_fts_content` view — headers AND every already-cached body.
    ///
    /// Idempotent and cheap when there is nothing to do (two EXISTS probes);
    /// on a database that was never migrated the index is trigger-maintained
    /// and non-empty, so the rebuild never re-runs. Run as a deferred
    /// post-startup task by the composition root (the address-book-backfill
    /// pattern), off the hot open path. Returns whether a rebuild ran.
    pub fn backfill_message_fts(&self) -> Result<bool, StoreError> {
        self.write_transaction(|tx| {
            let has_messages: bool = tx
                .query_row("SELECT EXISTS(SELECT 1 FROM message)", [], |row| row.get(0))
                .map_err(sql_to_store_error)?;
            // NOTE: probed via the `%_docsize` FTS5 shadow table (one row per
            // *indexed* document), NOT `SELECT … FROM message_fts` — a
            // non-MATCH scan of an external-content table reads through the
            // content view, so it would always look populated even when the
            // inverted index itself is empty.
            let has_index_rows: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM message_fts_docsize)",
                    [],
                    |row| row.get(0),
                )
                .map_err(sql_to_store_error)?;
            if !has_messages || has_index_rows {
                return Ok(false);
            }
            tx.execute("INSERT INTO message_fts(message_fts) VALUES('rebuild')", [])
                .map_err(sql_to_store_error)?;
            Ok(true)
        })
    }
}
