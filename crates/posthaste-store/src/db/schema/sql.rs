pub(super) const SCHEMA_SQL: &str = "
            CREATE TABLE IF NOT EXISTS mailbox (
                account_id TEXT NOT NULL,
                id TEXT NOT NULL,
                name TEXT NOT NULL,
                role TEXT,
                unread_emails INTEGER NOT NULL DEFAULT 0,
                total_emails INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, id)
            );

            CREATE TABLE IF NOT EXISTS mailbox_role_override (
                account_id TEXT NOT NULL,
                mailbox_id TEXT NOT NULL,
                role TEXT,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, mailbox_id)
            );

            CREATE TABLE IF NOT EXISTS message (
                account_id TEXT NOT NULL,
                id TEXT NOT NULL,
                thread_id TEXT NOT NULL,
                conversation_id TEXT,
                remote_blob_id TEXT,
                subject TEXT,
                normalized_subject TEXT,
                from_name TEXT,
                from_email TEXT,
                to_json TEXT NOT NULL DEFAULT '[]',
                preview TEXT,
                received_at TEXT NOT NULL,
                has_attachment INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0,
                is_read INTEGER NOT NULL DEFAULT 1,
                is_flagged INTEGER NOT NULL DEFAULT 0,
                rfc_message_id TEXT,
                in_reply_to TEXT,
                references_json TEXT NOT NULL DEFAULT '[]',
                draft_id TEXT,
                PRIMARY KEY (account_id, id)
            );

            CREATE TABLE IF NOT EXISTS conversation (
                id TEXT PRIMARY KEY,
                subject TEXT,
                normalized_subject TEXT,
                latest_received_at TEXT NOT NULL,
                latest_source_id TEXT NOT NULL,
                latest_message_id TEXT NOT NULL,
                message_count INTEGER NOT NULL DEFAULT 0,
                unread_count INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS conversation_message (
                conversation_id TEXT NOT NULL,
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                PRIMARY KEY (conversation_id, account_id, message_id)
            );

            CREATE TABLE IF NOT EXISTS message_mailbox (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                mailbox_id TEXT NOT NULL,
                PRIMARY KEY (account_id, message_id, mailbox_id)
            );

            CREATE TABLE IF NOT EXISTS message_keyword (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                keyword TEXT NOT NULL,
                PRIMARY KEY (account_id, message_id, keyword)
            );

            CREATE TABLE IF NOT EXISTS message_body (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                body_html TEXT,
                body_text TEXT,
                raw_path TEXT,
                raw_sha256 TEXT,
                raw_size INTEGER,
                raw_mime_type TEXT,
                fetched_at TEXT,
                PRIMARY KEY (account_id, message_id)
            );

            CREATE TABLE IF NOT EXISTS message_attachment (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                id TEXT NOT NULL,
                blob_id TEXT NOT NULL,
                part_id TEXT,
                filename TEXT,
                mime_type TEXT NOT NULL,
                size INTEGER NOT NULL DEFAULT 0,
                disposition TEXT,
                cid TEXT,
                is_inline INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, message_id, id)
            );

            CREATE TABLE IF NOT EXISTS thread_view (
                account_id TEXT NOT NULL,
                id TEXT NOT NULL,
                email_ids TEXT NOT NULL,
                PRIMARY KEY (account_id, id)
            );

            CREATE TABLE IF NOT EXISTS sync_cursor (
                account_id TEXT NOT NULL,
                object_type TEXT NOT NULL,
                state TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, object_type)
            );

            CREATE TABLE IF NOT EXISTS imap_mailbox_sync_state (
                account_id TEXT NOT NULL,
                mailbox_id TEXT NOT NULL,
                mailbox_name TEXT NOT NULL,
                uid_validity INTEGER NOT NULL,
                highest_uid INTEGER,
                highest_modseq TEXT,
                partial_initial_uid INTEGER,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, mailbox_id)
            );

            CREATE TABLE IF NOT EXISTS imap_message_location (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                mailbox_id TEXT NOT NULL,
                uid_validity INTEGER NOT NULL,
                uid INTEGER NOT NULL,
                modseq TEXT,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, message_id, mailbox_id)
            );

            CREATE TABLE IF NOT EXISTS event_log (
                seq INTEGER PRIMARY KEY AUTOINCREMENT,
                account_id TEXT NOT NULL,
                topic TEXT NOT NULL,
                occurred_at TEXT NOT NULL,
                mailbox_id TEXT,
                message_id TEXT,
                payload TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS source_projection (
                source_id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS draft_alias (
                account_id TEXT NOT NULL,
                draft_key TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                PRIMARY KEY (account_id, draft_key)
            );

            CREATE TABLE IF NOT EXISTS outbox_operation (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL,
                entity_kind TEXT NOT NULL,
                entity_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                payload TEXT NOT NULL,
                state TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                depends_on TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS automation_backfill_job (
                account_id TEXT NOT NULL,
                rule_fingerprint TEXT NOT NULL,
                status TEXT NOT NULL,
                attempts INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                queued_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                PRIMARY KEY (account_id, rule_fingerprint)
            );

            CREATE TABLE IF NOT EXISTS address_book (
                account_id TEXT NOT NULL,
                normalized_email TEXT NOT NULL,
                email TEXT NOT NULL,
                name TEXT,
                frequency INTEGER NOT NULL DEFAULT 0,
                last_seen_at TEXT NOT NULL,
                PRIMARY KEY (account_id, normalized_email)
            );

            CREATE TABLE IF NOT EXISTS cache_object (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                layer TEXT NOT NULL,
                object_id TEXT NOT NULL DEFAULT '',
                fetch_unit TEXT NOT NULL,
                state TEXT NOT NULL,
                value_bytes INTEGER NOT NULL DEFAULT 0,
                fetch_bytes INTEGER NOT NULL DEFAULT 0,
                priority REAL NOT NULL DEFAULT 0,
                reason TEXT NOT NULL DEFAULT '',
                last_scored_at TEXT NOT NULL,
                last_accessed_at TEXT,
                fetched_at TEXT,
                error_code TEXT,
                PRIMARY KEY (account_id, message_id, layer, object_id),
                FOREIGN KEY (account_id, message_id)
                    REFERENCES message(account_id, id)
                    ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS cache_message_signal (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                search_total_messages INTEGER,
                search_result_count INTEGER,
                search_result_rank INTEGER,
                search_seen_count INTEGER NOT NULL DEFAULT 0,
                last_search_seen_at TEXT,
                thread_activity_score REAL,
                sender_affinity_score REAL,
                local_behavior_score REAL,
                direct_user_boost REAL,
                pinned INTEGER,
                dirty_at TEXT,
                PRIMARY KEY (account_id, message_id),
                FOREIGN KEY (account_id, message_id)
                    REFERENCES message(account_id, id)
                    ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS cache_rescore_queue (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                reason TEXT NOT NULL,
                queued_at TEXT NOT NULL,
                rescore_priority REAL NOT NULL DEFAULT 0,
                PRIMARY KEY (account_id, message_id),
                FOREIGN KEY (account_id, message_id)
                    REFERENCES message(account_id, id)
                    ON DELETE CASCADE
            );

            -- Phase 2 undo/redo: the per-account server-authoritative reversible-op
            -- log + cursor. Append-only on forward actions; the cursor is mutable.
            -- `diff` is a MessageChangeDiff JSON (opaque to the store). See
            -- docs/eph/DESIGN-L2-undo-redo-revlog-contract.
            CREATE TABLE IF NOT EXISTS rev_log (
                account_id TEXT NOT NULL,
                step_id TEXT NOT NULL,
                seq INTEGER NOT NULL,
                message_id TEXT NOT NULL,
                source_id TEXT NOT NULL,
                diff TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (account_id, step_id)
            );
            CREATE TABLE IF NOT EXISTS rev_cursor (
                account_id TEXT PRIMARY KEY,
                cursor_step_id TEXT,
                redo_tail TEXT NOT NULL DEFAULT '[]'
            );

            CREATE INDEX IF NOT EXISTS idx_message_thread
                ON message (account_id, thread_id, received_at);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_mailbox_role_override_unique_role
                ON mailbox_role_override (account_id, role)
                WHERE role IS NOT NULL;
            CREATE INDEX IF NOT EXISTS idx_message_account_received
                ON message (account_id, received_at, id);
            CREATE INDEX IF NOT EXISTS idx_message_account_from_sort
                ON message (account_id, LOWER(COALESCE(from_name, from_email, '')), id);
            CREATE INDEX IF NOT EXISTS idx_message_account_subject_sort
                ON message (account_id, LOWER(COALESCE(subject, '')), id);
            CREATE INDEX IF NOT EXISTS idx_message_account_flagged_sort
                ON message (account_id, is_flagged, id);
            CREATE INDEX IF NOT EXISTS idx_message_account_attachment_sort
                ON message (account_id, has_attachment, id);
            CREATE INDEX IF NOT EXISTS idx_message_conversation
                ON message (conversation_id, received_at);
            CREATE INDEX IF NOT EXISTS idx_message_rfc_message_id
                ON message (rfc_message_id);
            CREATE INDEX IF NOT EXISTS idx_message_mailbox
                ON message_mailbox (account_id, mailbox_id);
            CREATE INDEX IF NOT EXISTS idx_message_keyword
                ON message_keyword (account_id, keyword);
            CREATE INDEX IF NOT EXISTS idx_message_attachment_blob
                ON message_attachment (account_id, blob_id);
            CREATE INDEX IF NOT EXISTS idx_outbox_operation_account_entity
                ON outbox_operation (account_id, entity_id);
            CREATE INDEX IF NOT EXISTS idx_event_log_lookup
                ON event_log (account_id, topic, mailbox_id, seq);
            CREATE INDEX IF NOT EXISTS idx_outbox_account_state
                ON outbox_operation (account_id, state);
            CREATE INDEX IF NOT EXISTS idx_draft_alias_entity
                ON draft_alias (account_id, entity_id);
            CREATE INDEX IF NOT EXISTS idx_conversation_message_lookup
                ON conversation_message (account_id, message_id);
            CREATE INDEX IF NOT EXISTS idx_automation_backfill_pending
                ON automation_backfill_job (account_id, status, updated_at);
            CREATE INDEX IF NOT EXISTS idx_address_book_rank
                ON address_book (frequency DESC, last_seen_at DESC, account_id, normalized_email);
            CREATE INDEX IF NOT EXISTS idx_cache_fetch_candidates
                ON cache_object (account_id, state, layer, priority DESC);
            CREATE INDEX IF NOT EXISTS idx_cache_cached_bytes
                ON cache_object (state, fetch_bytes);
            CREATE INDEX IF NOT EXISTS idx_cache_signal_dirty
                ON cache_message_signal (account_id, dirty_at);
            CREATE INDEX IF NOT EXISTS idx_cache_rescore_queue
                ON cache_rescore_queue (account_id, queued_at);
            CREATE INDEX IF NOT EXISTS idx_rev_log_account_seq
                ON rev_log (account_id, seq);

            -- Full-text search index over message header fields AND the cached
            -- body text. External-content FTS5 table (only the inverted index is
            -- stored; column values are read back through the content view, so
            -- body text is never duplicated) keyed by message.rowid. The view
            -- joins the header row with its body-cache row (message_body,
            -- body_text) — a message with no cached body indexes a NULL body and
            -- becomes body-searchable when the cache warms it.
            --
            -- Sync is trigger-maintained from BOTH base tables. FTS5
            -- external-content integrity rule: every 'delete' command must carry
            -- exactly the values that were last inserted for that rowid. The
            -- triggers below therefore keep one invariant: the indexed body for a
            -- message.rowid always equals the CURRENT message_body.body_text (or
            -- NULL when no row). Each message_body mutation re-indexes its
            -- message row; each message mutation reads the live body via
            -- subselect. Message deletes work in either order: if message_body
            -- goes first its trigger re-indexes with a NULL body and the later
            -- message trigger deletes with a NULL subselect; if message goes
            -- first its trigger deletes with the still-present body and the
            -- message_body trigger is a no-op (guarded on the message row).
            CREATE VIEW IF NOT EXISTS message_fts_content AS
                SELECT m.rowid AS rowid, m.subject, m.from_name, m.from_email, m.preview,
                       (SELECT b.body_text FROM message_body b
                         WHERE b.account_id = m.account_id AND b.message_id = m.id) AS body
                FROM message m;
            CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
                subject, from_name, from_email, preview, body,
                content='message_fts_content', content_rowid='rowid',
                tokenize='porter unicode61 remove_diacritics 2'
            );
            CREATE TRIGGER IF NOT EXISTS message_fts_ai AFTER INSERT ON message BEGIN
                INSERT INTO message_fts(rowid, subject, from_name, from_email, preview, body)
                VALUES (new.rowid, new.subject, new.from_name, new.from_email, new.preview,
                        (SELECT body_text FROM message_body
                          WHERE account_id = new.account_id AND message_id = new.id));
            END;
            CREATE TRIGGER IF NOT EXISTS message_fts_ad AFTER DELETE ON message BEGIN
                INSERT INTO message_fts(message_fts, rowid, subject, from_name, from_email, preview, body)
                VALUES ('delete', old.rowid, old.subject, old.from_name, old.from_email, old.preview,
                        (SELECT body_text FROM message_body
                          WHERE account_id = old.account_id AND message_id = old.id));
            END;
            CREATE TRIGGER IF NOT EXISTS message_fts_au AFTER UPDATE ON message BEGIN
                INSERT INTO message_fts(message_fts, rowid, subject, from_name, from_email, preview, body)
                VALUES ('delete', old.rowid, old.subject, old.from_name, old.from_email, old.preview,
                        (SELECT body_text FROM message_body
                          WHERE account_id = old.account_id AND message_id = old.id));
                INSERT INTO message_fts(rowid, subject, from_name, from_email, preview, body)
                VALUES (new.rowid, new.subject, new.from_name, new.from_email, new.preview,
                        (SELECT body_text FROM message_body
                          WHERE account_id = new.account_id AND message_id = new.id));
            END;
            -- Body-cache writes re-index the owning message row the moment a
            -- body lands (the fetch path and the sync path both funnel through
            -- one message_body upsert). Guarded on the message row existing:
            -- a body row without its header row has no FTS row to maintain.
            CREATE TRIGGER IF NOT EXISTS message_body_fts_ai AFTER INSERT ON message_body
            WHEN EXISTS (SELECT 1 FROM message m
                          WHERE m.account_id = new.account_id AND m.id = new.message_id)
            BEGIN
                INSERT INTO message_fts(message_fts, rowid, subject, from_name, from_email, preview, body)
                SELECT 'delete', m.rowid, m.subject, m.from_name, m.from_email, m.preview, NULL
                FROM message m
                WHERE m.account_id = new.account_id AND m.id = new.message_id;
                INSERT INTO message_fts(rowid, subject, from_name, from_email, preview, body)
                SELECT m.rowid, m.subject, m.from_name, m.from_email, m.preview, new.body_text
                FROM message m
                WHERE m.account_id = new.account_id AND m.id = new.message_id;
            END;
            CREATE TRIGGER IF NOT EXISTS message_body_fts_au AFTER UPDATE ON message_body
            WHEN EXISTS (SELECT 1 FROM message m
                          WHERE m.account_id = new.account_id AND m.id = new.message_id)
            BEGIN
                INSERT INTO message_fts(message_fts, rowid, subject, from_name, from_email, preview, body)
                SELECT 'delete', m.rowid, m.subject, m.from_name, m.from_email, m.preview, old.body_text
                FROM message m
                WHERE m.account_id = old.account_id AND m.id = old.message_id;
                INSERT INTO message_fts(rowid, subject, from_name, from_email, preview, body)
                SELECT m.rowid, m.subject, m.from_name, m.from_email, m.preview, new.body_text
                FROM message m
                WHERE m.account_id = new.account_id AND m.id = new.message_id;
            END;
            CREATE TRIGGER IF NOT EXISTS message_body_fts_ad AFTER DELETE ON message_body
            WHEN EXISTS (SELECT 1 FROM message m
                          WHERE m.account_id = old.account_id AND m.id = old.message_id)
            BEGIN
                INSERT INTO message_fts(message_fts, rowid, subject, from_name, from_email, preview, body)
                SELECT 'delete', m.rowid, m.subject, m.from_name, m.from_email, m.preview, old.body_text
                FROM message m
                WHERE m.account_id = old.account_id AND m.id = old.message_id;
                INSERT INTO message_fts(rowid, subject, from_name, from_email, preview, body)
                SELECT m.rowid, m.subject, m.from_name, m.from_email, m.preview, NULL
                FROM message m
                WHERE m.account_id = old.account_id AND m.id = old.message_id;
            END;

            -- Incrementally maintain mailbox counters. This replaces expensive
            -- full recounts after each changed mailbox with O(changed rows)
            -- updates tied directly to the underlying membership/read-state
            -- mutations.
            CREATE TRIGGER IF NOT EXISTS mailbox_counters_message_mailbox_ai
            AFTER INSERT ON message_mailbox BEGIN
                UPDATE mailbox
                   SET total_emails = total_emails + 1,
                       unread_emails = unread_emails + CASE WHEN COALESCE((
                           SELECT is_read FROM message
                            WHERE account_id = new.account_id AND id = new.message_id
                       ), 1) = 0 THEN 1 ELSE 0 END
                 WHERE account_id = new.account_id AND id = new.mailbox_id;
            END;
            CREATE TRIGGER IF NOT EXISTS mailbox_counters_message_mailbox_ad
            AFTER DELETE ON message_mailbox BEGIN
                UPDATE mailbox
                   SET total_emails = CASE WHEN total_emails > 0 THEN total_emails - 1 ELSE 0 END,
                       unread_emails = CASE
                           WHEN COALESCE((
                               SELECT is_read FROM message
                                WHERE account_id = old.account_id AND id = old.message_id
                           ), 1) = 0 AND unread_emails > 0 THEN unread_emails - 1
                           ELSE unread_emails
                       END
                 WHERE account_id = old.account_id AND id = old.mailbox_id;
            END;
            CREATE TRIGGER IF NOT EXISTS mailbox_counters_message_read_au
            AFTER UPDATE OF is_read ON message
            WHEN old.is_read != new.is_read BEGIN
                UPDATE mailbox
                   SET unread_emails = CASE
                       WHEN new.is_read = 0 THEN unread_emails + 1
                       WHEN unread_emails > 0 THEN unread_emails - 1
                       ELSE 0
                   END
                 WHERE account_id = new.account_id
                   AND id IN (
                       SELECT mailbox_id FROM message_mailbox
                        WHERE account_id = new.account_id AND message_id = new.id
                   );
            END;

            -- Snooze: a Posthaste-local return-time for a message in the Snoozed
            -- mailbox. Not provider-synced (providers have no snooze field).
            -- The scheduler (supervisor snooze tick) scans this for due rows.
            -- @spec docs/eph/DESIGN-L2-snooze
            CREATE TABLE IF NOT EXISTS message_snooze (
                account_id TEXT NOT NULL,
                message_id TEXT NOT NULL,
                until INTEGER NOT NULL,
                PRIMARY KEY (account_id, message_id)
            );
            CREATE INDEX IF NOT EXISTS idx_message_snooze_until
                ON message_snooze (account_id, until);

            -- DS7: the durable direct-apply idempotency ledger. One row per
            -- (caller scope, idempotency key): reserved 'pending' BEFORE the
            -- keyed operation executes, settled to 'confirmed'/'rejected' with
            -- the outcome JSON after. The durable source of truth for \"already
            -- applied\" — a redelivery after a process restart (or after the
            -- in-memory ledger's TTL reap) finds the prior decision here and is
            -- never re-executed. Caller-scoped (not account-scoped): the key is
            -- the client-supplied Idempotency-Key under its ApplyScope bucket,
            -- mirroring the in-memory ledger's (ApplyScope, ClientMutationId).
            -- Settled rows are GC'd only past APPLY_LEDGER_RETENTION_SECS (see
            -- src/apply_ledger.rs — it dominates any realistic redelivery
            -- window); 'pending' rows are never GC'd (an unresolved crash
            -- marker must keep blocking re-execution).
            CREATE TABLE IF NOT EXISTS apply_ledger (
                scope TEXT NOT NULL,
                idempotency_key TEXT NOT NULL,
                op_name TEXT NOT NULL,
                state TEXT NOT NULL,
                outcome_json TEXT,
                created_at INTEGER NOT NULL,
                settled_at INTEGER,
                PRIMARY KEY (scope, idempotency_key)
            );
            CREATE INDEX IF NOT EXISTS idx_apply_ledger_settled_at
                ON apply_ledger (settled_at)
                WHERE settled_at IS NOT NULL;
            ";
