//! Re-deriving message metadata from the raw MIME the body cache already holds.
//!
//! Every field on a message summary that is parsed out of the header block is
//! a field some already-stored row can be missing: it lands in a release, and
//! mail synced before that release keeps a NULL/empty column forever, because
//! delta sync never re-fetches an unchanged message. The at-open fill closes
//! most of that — a body fetch re-serves the full headers — but only for mail
//! whose body is fetched *after* the upgrade. Mail whose body was already
//! cached never fetches again, so it never fills.
//!
//! The saving structural fact is that those two populations are the same set:
//! a cached body means a retained `.eml` on disk (`message_body.raw_path`),
//! which is the same bytes the provider would re-serve. So the repair is
//! network-free, and complete for everything it can reach.
//!
//! The derivation itself is [`posthaste_domain_service::DerivedMessageMetadata`]
//! — the one shared with the body-fetch path, so this pass cannot disagree with
//! the at-open fill about what a message's Cc is. What lives here is the
//! mapping from derived fields to message columns
//! ([`DERIVED_MESSAGE_COLUMNS`]), the non-clobbering write both paths share
//! ([`fill_derived_metadata_tx`]), and the chunked repair pass.
//!
//! ADDING A FIELD: add it to `DerivedMessageMetadata`, add one row to
//! [`DERIVED_MESSAGE_COLUMNS`], and bump [`DERIVED_METADATA_REVISION`]. The
//! bump re-arms the deferred pass exactly once per database, so already-cached
//! mail gains the new field on the next startup.

use posthaste_domain_service::{derive_message_metadata, DerivedMessageMetadata};

use super::*;
use crate::sql_cache::CachedSql;

/// The revision of [`DERIVED_MESSAGE_COLUMNS`]. Bumping it re-arms the
/// deferred pass on every existing database exactly once.
pub(crate) const DERIVED_METADATA_REVISION: i64 = 1;

/// The [`store_maintenance_marker`] key recording the highest
/// [`DERIVED_METADATA_REVISION`] this database has completed a pass for.
///
/// [`store_maintenance_marker`]: crate::db::schema
const DERIVED_METADATA_MARKER: &str = "derived_message_metadata_revision";

/// How many cached bodies one write transaction covers. Each row costs a file
/// read plus a MIME parse, both done OUTSIDE the transaction; the txn itself
/// is a handful of primary-key updates. Small enough that the store's global
/// write lock is never held for long, large enough that a big cache does not
/// pay per-row transaction overhead.
pub(crate) const REDERIVE_CHUNK: usize = 200;

/// Reads a derived field back out as the JSON its message column stores, or
/// `None` when the derivation found nothing for it.
type DerivedColumnValue = fn(&DerivedMessageMetadata) -> Option<String>;

/// The derivation table: which message column each derived field fills.
///
/// `None` means "nothing derived" and is never written — see
/// [`fill_derived_metadata_tx`]. A serialization failure degrades to `None`
/// for the same reason: these are best-effort repairs of one column, and no
/// single field is worth failing a whole pass over.
const DERIVED_MESSAGE_COLUMNS: &[(&str, DerivedColumnValue)] = &[
    ("cc_json", |derived| recipients_json(&derived.cc)),
    ("bcc_json", |derived| recipients_json(&derived.bcc)),
    ("reply_to_json", |derived| {
        recipients_json(&derived.reply_to)
    }),
    ("list_unsubscribe", |derived| {
        derived
            .list_unsubscribe
            .as_ref()
            .and_then(|targets| serde_json::to_string(targets).ok())
    }),
];

fn recipients_json(recipients: &[Recipient]) -> Option<String> {
    if recipients.is_empty() {
        return None;
    }
    serde_json::to_string(recipients).ok()
}

/// What one re-derive pass did. Counts, not identities: the pass is background
/// repair, and the interesting question is only ever "did anything change".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MetadataRederiveReport {
    /// Cached bodies considered.
    pub examined: u64,
    /// Messages that gained at least one column.
    pub filled: u64,
    /// Cached bodies whose `.eml` was missing, unreadable, or unparseable.
    /// Skipped, never fatal — a repair that aborts on one bad object is worse
    /// than one that reports partial progress.
    pub unreadable: u64,
}

/// One cached raw body to consider: enough to find the file and the row.
struct CachedRawBody {
    account_id: AccountId,
    message_id: MessageId,
    raw_path: String,
}

/// Fills every message column derivable from `derived` that is still empty,
/// and returns whether anything was written.
///
/// The one write path for derived metadata — shared by the body-fetch fill and
/// the repair pass, so the two cannot drift.
///
/// NEVER clobbers: the `IS NULL OR = '[]'` guard means a value some earlier
/// parse already stored always wins, and a derivation that found nothing
/// (`None`) is not written at all. Both directions matter, because these
/// headers are immutable per message: an empty derived value means "this
/// source carried no such header", never "the header was removed".
pub(crate) fn fill_derived_metadata_tx(
    tx: &Transaction<'_>,
    account_id: &AccountId,
    message_id: &MessageId,
    derived: &DerivedMessageMetadata,
) -> Result<bool, StoreError> {
    let mut filled = false;
    for (column, value_of) in DERIVED_MESSAGE_COLUMNS {
        let Some(value) = value_of(derived) else {
            continue;
        };
        let updated = tx
            .execute_cached(
                &format!(
                    "UPDATE message SET {column} = ?3
                     WHERE account_id = ?1 AND id = ?2
                       AND ({column} IS NULL OR {column} = '[]')"
                ),
                params![account_id.as_str(), message_id.as_str(), value],
            )
            .map_err(sql_to_store_error)?;
        filled |= updated > 0;
    }
    Ok(filled)
}

impl DatabaseStore {
    /// The deferred-startup entry point: re-derive metadata for every cached
    /// body, but only if this database has not already completed a pass at the
    /// current [`DERIVED_METADATA_REVISION`]. `None` means the guard
    /// short-circuited and nothing was touched.
    ///
    /// The guard is ONE primary-key probe of `store_maintenance_marker`,
    /// which is what makes this safe to run on every startup. It cannot be
    /// derived from the data instead: "this row has not been re-derived yet"
    /// and "this row legitimately has no Cc" are the same empty column, and
    /// the second describes most mail — there is no predicate to index. So the
    /// fact that the pass ran is recorded rather than inferred.
    ///
    /// Failure leaves the marker unset, so an interrupted or failed pass is
    /// simply re-run on the next startup; the pass writes nothing that a
    /// re-run would double-apply.
    pub fn rederive_stale_message_metadata(
        &self,
    ) -> Result<Option<MetadataRederiveReport>, StoreError> {
        if self.completed_derived_metadata_revision()? >= DERIVED_METADATA_REVISION {
            return Ok(None);
        }
        let report = self.rederive_message_metadata()?;
        self.record_derived_metadata_revision()?;
        Ok(Some(report))
    }

    /// The manual entry point (Settings → Troubleshooting): re-derive metadata
    /// for every cached body regardless of the marker.
    ///
    /// Unguarded on purpose. By the time a user reaches for this, the deferred
    /// pass has almost certainly already run and set the marker, so a guarded
    /// button would do nothing precisely when it is asked for. Re-running is
    /// safe and quiet: the derivation is a pure function of bytes already on
    /// disk, and the write refuses to overwrite anything, so a second run
    /// re-reads and changes nothing.
    ///
    /// Global scope, matching the body-cache repair beside it: a cached body
    /// is keyed by account but the repair has no account-specific failure mode
    /// and the user is not told which account is stale.
    pub fn rederive_message_metadata(&self) -> Result<MetadataRederiveReport, StoreError> {
        let mut report = MetadataRederiveReport::default();
        // Keyset pagination over `message_body`'s primary key rather than
        // OFFSET: the cursor is stable under the concurrent body writes that
        // keep happening while this runs, and every page is an index seek.
        let mut cursor = (String::new(), String::new());
        loop {
            let batch = self.cached_raw_bodies_after(&cursor, REDERIVE_CHUNK)?;
            let Some(last) = batch.last() else {
                break;
            };
            cursor = (
                last.account_id.as_str().to_string(),
                last.message_id.as_str().to_string(),
            );
            self.apply_rederive_batch(&batch, &mut report)?;
        }
        Ok(report)
    }

    /// One page of cached raw bodies, ordered by primary key, strictly after
    /// `cursor`. Ids are never empty, so `("", "")` starts at the beginning.
    fn cached_raw_bodies_after(
        &self,
        cursor: &(String, String),
        limit: usize,
    ) -> Result<Vec<CachedRawBody>, StoreError> {
        let connection = self.read_connection()?;
        let mut statement = connection
            .prepare_cached(
                "SELECT account_id, message_id, raw_path
                 FROM message_body
                 WHERE raw_path IS NOT NULL
                   AND (account_id > ?1 OR (account_id = ?1 AND message_id > ?2))
                 ORDER BY account_id, message_id
                 LIMIT ?3",
            )
            .map_err(sql_to_store_error)?;
        let rows = statement
            .query_map(params![cursor.0, cursor.1, limit as i64], |row| {
                Ok(CachedRawBody {
                    account_id: AccountId(row.get(0)?),
                    message_id: MessageId(row.get(1)?),
                    raw_path: row.get(2)?,
                })
            })
            .map_err(sql_to_store_error)?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(sql_to_store_error)
    }

    /// Reads and parses one page OFF the write lock, then applies whatever it
    /// derived in a single short transaction. File I/O and MIME parsing are the
    /// expensive half and must never happen inside the txn — the store's write
    /// connection is a global choke point that the overlay fold also queues on.
    fn apply_rederive_batch(
        &self,
        batch: &[CachedRawBody],
        report: &mut MetadataRederiveReport,
    ) -> Result<(), StoreError> {
        let derived = batch
            .iter()
            .map(|row| derive_from_cached_raw(&row.raw_path))
            .collect::<Vec<_>>();
        report.examined += batch.len() as u64;
        report.unreadable += derived.iter().filter(|entry| entry.is_none()).count() as u64;
        // Nothing derivable in this whole page (all unreadable, or all plain
        // one-to-one mail): skip the transaction entirely rather than opening
        // one to run zero updates.
        if !derived
            .iter()
            .any(|entry| entry.as_ref().is_some_and(|value| !value.is_empty()))
        {
            return Ok(());
        }
        let filled = self.write_transaction(|tx| {
            let mut filled = 0u64;
            for (row, derived) in batch.iter().zip(derived.iter()) {
                let Some(derived) = derived else { continue };
                if fill_derived_metadata_tx(tx, &row.account_id, &row.message_id, derived)? {
                    filled += 1;
                }
            }
            Ok(filled)
        })?;
        report.filled += filled;
        Ok(())
    }

    /// The cheap guard: one primary-key probe. A database that has never run
    /// the pass has no row, which reads as revision 0.
    fn completed_derived_metadata_revision(&self) -> Result<i64, StoreError> {
        let connection = self.read_connection()?;
        let revision = connection
            .query_row(
                "SELECT value FROM store_maintenance_marker WHERE key = ?1",
                params![DERIVED_METADATA_MARKER],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(sql_to_store_error)?;
        Ok(revision.unwrap_or(0))
    }

    fn record_derived_metadata_revision(&self) -> Result<(), StoreError> {
        let updated_at = now_iso8601()?;
        self.write_transaction(|tx| {
            tx.execute(
                "INSERT INTO store_maintenance_marker (key, value, updated_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET
                    value = MAX(store_maintenance_marker.value, excluded.value),
                    updated_at = excluded.updated_at",
                params![
                    DERIVED_METADATA_MARKER,
                    DERIVED_METADATA_REVISION,
                    updated_at
                ],
            )
            .map_err(sql_to_store_error)?;
            Ok(())
        })
    }
}

/// Derives from one cached `.eml`, or `None` when the file is gone, unreadable,
/// or not a parseable message.
///
/// All three are ordinary rather than exceptional: the cache prunes files out
/// from under their rows, and a raw body is only retained when the fetched
/// bytes were valid UTF-8, so what is on disk is not guaranteed to round-trip.
/// The row is skipped and the pass continues.
fn derive_from_cached_raw(raw_path: &str) -> Option<DerivedMessageMetadata> {
    let raw = fs::read(raw_path).ok()?;
    derive_message_metadata(&raw)
}
