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
                base_cursor TEXT,
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

            CREATE TABLE IF NOT EXISTS sender_address_cache (
                account_id TEXT NOT NULL,
                normalized_email TEXT NOT NULL,
                email TEXT NOT NULL,
                name TEXT,
                last_used_at TEXT NOT NULL,
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
            CREATE INDEX IF NOT EXISTS idx_sender_address_cache_recent
                ON sender_address_cache (last_used_at DESC, account_id);
            CREATE INDEX IF NOT EXISTS idx_cache_fetch_candidates
                ON cache_object (account_id, state, layer, priority DESC);
            CREATE INDEX IF NOT EXISTS idx_cache_cached_bytes
                ON cache_object (state, fetch_bytes);
            CREATE INDEX IF NOT EXISTS idx_cache_signal_dirty
                ON cache_message_signal (account_id, dirty_at);
            CREATE INDEX IF NOT EXISTS idx_cache_rescore_queue
                ON cache_rescore_queue (account_id, queued_at);

            -- Full-text search index over message header fields. External-content
            -- FTS5 table linked to message.rowid; kept in sync by triggers so the
            -- write path needs no changes. (Prototype: body_text lives in
            -- message_body and is not yet indexed here.)
            CREATE VIRTUAL TABLE IF NOT EXISTS message_fts USING fts5(
                subject, from_name, from_email, preview,
                content='message', content_rowid='rowid',
                tokenize='porter unicode61 remove_diacritics 2'
            );
            CREATE TRIGGER IF NOT EXISTS message_fts_ai AFTER INSERT ON message BEGIN
                INSERT INTO message_fts(rowid, subject, from_name, from_email, preview)
                VALUES (new.rowid, new.subject, new.from_name, new.from_email, new.preview);
            END;
            CREATE TRIGGER IF NOT EXISTS message_fts_ad AFTER DELETE ON message BEGIN
                INSERT INTO message_fts(message_fts, rowid, subject, from_name, from_email, preview)
                VALUES ('delete', old.rowid, old.subject, old.from_name, old.from_email, old.preview);
            END;
            CREATE TRIGGER IF NOT EXISTS message_fts_au AFTER UPDATE ON message BEGIN
                INSERT INTO message_fts(message_fts, rowid, subject, from_name, from_email, preview)
                VALUES ('delete', old.rowid, old.subject, old.from_name, old.from_email, old.preview);
                INSERT INTO message_fts(rowid, subject, from_name, from_email, preview)
                VALUES (new.rowid, new.subject, new.from_name, new.from_email, new.preview);
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
            ";
