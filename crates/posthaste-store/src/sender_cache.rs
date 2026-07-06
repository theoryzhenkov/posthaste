use std::collections::HashMap;

use super::*;

impl SenderAddressCacheStore for DatabaseStore {
    /// The complete, ranked address book across every configured account.
    ///
    /// This is no longer a 40-slot send-time cache: it is the persistent
    /// address book maintained by [`harvest_addresses_tx`] on message ingest
    /// (senders *and* recipients), seeded once by [`Self::backfill_address_book`],
    /// and topped up by [`Self::remember_sender_address`] on send. Rows are
    /// ranked by correspondence frequency, then recency, so the highest-affinity
    /// correspondents surface first for autocomplete. There is no cap.
    fn list_sender_address_cache(&self) -> Result<Vec<CachedSenderAddress>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT account_id, name, email, last_seen_at
                 FROM address_book
                 ORDER BY frequency DESC, last_seen_at DESC, account_id ASC, normalized_email ASC",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok(CachedSenderAddress {
                    source_id: AccountId::from(row.get::<_, String>(0)?),
                    name: row.get(1)?,
                    email: row.get(2)?,
                    last_used_at: row.get(3)?,
                })
            })
            .map_err(sql_to_store_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)
    }

    /// Send-time contributor: a sent message's recipients (and the chosen From)
    /// are high-affinity, so keep harvesting them here — but this is now just
    /// *one* contributor to the book, not its only source, and it no longer
    /// prunes to 40.
    fn remember_sender_address(
        &self,
        account_id: &AccountId,
        sender: &Recipient,
    ) -> Result<(), StoreError> {
        let seen_at = now_iso8601()?;
        self.write_transaction(|tx| {
            upsert_address_tx(
                tx,
                account_id,
                &sender.email,
                sender.name.as_deref(),
                &seen_at,
            )
        })
    }
}

impl DatabaseStore {
    /// One-time backfill: populate the address book from every message already
    /// in the store (each message's `from` sender plus every `to` recipient),
    /// so the book is complete from day one rather than only reflecting mail
    /// that arrives after this feature ships.
    ///
    /// Idempotent and safe to run alongside live ingest: it computes the
    /// message-derived frequency/recency per correspondent and upserts with
    /// `MAX(...)` semantics, so re-running it (a future retry, or the next
    /// startup) never double-counts and never regresses a count that ingest has
    /// since advanced. Run as a deferred post-startup task by the composition
    /// root, off the hot open path (mirrors the body-cache repair).
    pub fn backfill_address_book(&self) -> Result<(), StoreError> {
        let candidates = self.collect_address_book_candidates()?;
        if candidates.is_empty() {
            return Ok(());
        }
        self.write_transaction(|tx| {
            for candidate in &candidates {
                tx.execute(
                    "INSERT INTO address_book (
                        account_id, normalized_email, email, name, frequency, last_seen_at
                     )
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(account_id, normalized_email) DO UPDATE SET
                        email = excluded.email,
                        name = COALESCE(excluded.name, address_book.name),
                        frequency = MAX(address_book.frequency, excluded.frequency),
                        last_seen_at = MAX(address_book.last_seen_at, excluded.last_seen_at)",
                    params![
                        candidate.account_id,
                        candidate.normalized_email,
                        candidate.email,
                        candidate.name,
                        candidate.frequency,
                        candidate.last_seen_at,
                    ],
                )
                .map_err(sql_to_store_error)?;
            }
            Ok(())
        })
    }

    /// Read pass of the backfill: stream every sender/recipient occurrence out of
    /// `message` and aggregate them in Rust (so the exact [`is_cacheable_sender_email`]
    /// validity filter applies), keeping the frequency, the most-recent
    /// `last_seen_at`, and the display name from the most recent occurrence that
    /// carried one.
    fn collect_address_book_candidates(&self) -> Result<Vec<AddressBookCandidate>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT account_id, from_email AS email, from_name AS name, received_at
                 FROM message
                 WHERE from_email IS NOT NULL AND TRIM(from_email) <> ''
                 UNION ALL
                 SELECT m.account_id,
                        json_extract(recipient.value, '$.email') AS email,
                        json_extract(recipient.value, '$.name') AS name,
                        m.received_at
                 FROM message m, json_each(m.to_json) AS recipient
                 WHERE json_extract(recipient.value, '$.email') IS NOT NULL
                   AND TRIM(json_extract(recipient.value, '$.email')) <> ''",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(sql_to_store_error)?;

        let mut aggregate: HashMap<(String, String), AddressBookCandidate> = HashMap::new();
        for row in rows {
            let (account_id, email, name, received_at) = row.map_err(sql_to_store_error)?;
            let email = email.trim();
            if !is_cacheable_sender_email(email) {
                continue;
            }
            let normalized_email = email.to_ascii_lowercase();
            let name = clean_display_name(name.as_deref());
            let entry = aggregate
                .entry((account_id.clone(), normalized_email.clone()))
                .or_insert_with(|| AddressBookCandidate {
                    account_id,
                    normalized_email,
                    email: email.to_string(),
                    name: None,
                    frequency: 0,
                    last_seen_at: String::new(),
                    name_seen_at: String::new(),
                });
            entry.frequency += 1;
            if received_at > entry.last_seen_at {
                entry.last_seen_at.clone_from(&received_at);
            }
            // Keep the display name from the most recent occurrence that has one.
            if let Some(name) = name {
                if entry.name.is_none() || received_at >= entry.name_seen_at {
                    entry.name = Some(name);
                    entry.name_seen_at = received_at;
                }
            }
        }
        Ok(aggregate.into_values().collect())
    }
}

/// Upsert a single correspondent into the address book: bump its frequency,
/// advance `last_seen_at`, and keep the best available display name. Shared by
/// the send-time contributor and the ingest harvest. Invalid addresses (the
/// existing [`is_cacheable_sender_email`] check) are silently skipped.
pub(crate) fn upsert_address_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    email: &str,
    name: Option<&str>,
    seen_at: &str,
) -> Result<(), StoreError> {
    let email = email.trim();
    if !is_cacheable_sender_email(email) {
        return Ok(());
    }
    let normalized_email = email.to_ascii_lowercase();
    let name = clean_display_name(name);
    tx.execute(
        "INSERT INTO address_book (
            account_id, normalized_email, email, name, frequency, last_seen_at
         )
         VALUES (?1, ?2, ?3, ?4, 1, ?5)
         ON CONFLICT(account_id, normalized_email) DO UPDATE SET
            email = excluded.email,
            name = COALESCE(excluded.name, address_book.name),
            frequency = address_book.frequency + 1,
            last_seen_at = MAX(address_book.last_seen_at, excluded.last_seen_at)",
        params![account_id.as_str(), normalized_email, email, name, seen_at],
    )
    .map_err(sql_to_store_error)?;
    Ok(())
}

/// Harvest every correspondent an ingested message contributes: its `from`
/// sender and each of its `to` recipients. De-duplicated by normalized email
/// within the message so one message counts once per distinct correspondent.
/// The message's `received_at` is used as the last-seen timestamp.
pub(crate) fn harvest_addresses_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    from_name: Option<&str>,
    from_email: Option<&str>,
    recipients: &[Recipient],
    received_at: &str,
) -> Result<(), StoreError> {
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut harvest = |email: Option<&str>, name: Option<&str>| -> Result<(), StoreError> {
        let Some(email) = email else {
            return Ok(());
        };
        let trimmed = email.trim();
        if trimmed.is_empty() {
            return Ok(());
        }
        let normalized = trimmed.to_ascii_lowercase();
        if seen.insert(normalized, ()).is_some() {
            return Ok(());
        }
        upsert_address_tx(tx, account_id, trimmed, name, received_at)
    };
    harvest(from_email, from_name)?;
    for recipient in recipients {
        harvest(Some(recipient.email.as_str()), recipient.name.as_deref())?;
    }
    Ok(())
}

fn clean_display_name(name: Option<&str>) -> Option<String> {
    name.map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

struct AddressBookCandidate {
    account_id: String,
    normalized_email: String,
    email: String,
    name: Option<String>,
    frequency: i64,
    last_seen_at: String,
    name_seen_at: String,
}

fn is_cacheable_sender_email(email: &str) -> bool {
    if email.is_empty()
        || email.contains('*')
        || email.chars().any(|character| character.is_whitespace())
    {
        return false;
    }
    let mut parts = email.split('@');
    matches!(
        (parts.next(), parts.next(), parts.next()),
        (Some(local), Some(domain), None) if !local.is_empty() && !domain.is_empty()
    )
}
