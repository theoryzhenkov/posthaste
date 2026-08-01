//! Local-store maintenance intents: repairs the user can ask for by hand when
//! the automatic, deferred passes have already run and something still looks
//! wrong.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Content for [`crate::Command::RederiveMessageMetadata`].
///
/// No fields, and global scope by design: the repair reads only bytes already
/// on disk (the retained raw MIME behind every cached body), fills only
/// columns that are still empty, and has no per-account failure mode the user
/// could be expected to name.
#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
pub struct RederiveMessageMetadataIntent {}
