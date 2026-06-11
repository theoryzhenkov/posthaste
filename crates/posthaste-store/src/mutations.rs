use super::*;
use crate::cache::{ensure_body_cache_object_tx, BACKGROUND_RESCORE_PRIORITY};
use crate::projections::{
    assign_conversation_id_tx, delete_message_tx, normalized_subject,
    refresh_conversation_projection_tx, refresh_mailbox_counters_tx, refresh_thread_projection_tx,
    replace_attachments_tx, upsert_body_tx,
};
use crate::query::{
    fetch_keywords_tx, fetch_mailbox_ids_tx, query_message_detail_tx, row_to_event,
};

/// Stages raw MIME bodies to disk before the write transaction so that file
/// I/O does not block the SQLite lock. Falls back to synthesizing a minimal
/// RFC 822 message when `raw_mime` is absent but body HTML/text is present.
mod commands;
mod events;
mod mailbox_cleanup;
mod message_apply;
mod projection_tracking;
mod sync_batch;
mod types;

pub(crate) use commands::{
    apply_message_body_tx, destroy_message_tx, replace_mailboxes_tx, set_keywords_tx,
};
pub(crate) use events::list_events;
pub(crate) use sync_batch::{apply_sync_batch_tx, stage_sync_bodies};

use mailbox_cleanup::{
    delete_mailbox_and_track_projection_inputs, prune_stale_imap_message_locations_for_snapshot_tx,
};
use message_apply::{
    apply_message_record_tx, effective_mailbox_role_tx, fetch_message_before_apply_tx,
};
use projection_tracking::{
    append_message_diff_events_tx, delete_imap_message_location_and_track_projection_inputs,
    delete_message_and_track_projection_inputs, track_applied_message_projection_inputs,
};
use types::{MessageBeforeApply, ProjectionInputs};
