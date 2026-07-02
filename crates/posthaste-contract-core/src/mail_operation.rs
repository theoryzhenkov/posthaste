//! The typed mail-operation vocabulary (RFC-L2-architecture-cleanup D8/D11/D22).
//!
//! `MailOperation` is the single, typed operation vocabulary every tier
//! (client, runtime near node, authority far node) parses once per wire and
//! carries typed inward — replacing the stringly `MutationRequest { name, args }`
//! dispatch and the per-crate message-mutation table it superseded (D11).
//! Dispatch becomes an exhaustive `match`, not a string lookup.
//!
//! The serde encoding is **adjacently tagged** — `{"name": "...", "args": {...}}`
//! — reusing the catalogued operation names as the tag values so logs, wire
//! payloads, and tests stay readable and the `args` sub-object matches the shape
//! each argument struct already carried.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use posthaste_link_core::{MessageAssertion, MessageChangeDiff};

use crate::mutation_args::{
    keyword_toggle, MessageApplyDiffArgs, MessageMailboxMembershipArgs, MessageMoveToMailboxArgs,
    MessageMoveToRoleArgs, MessageReplaceMailboxesArgs, MessageSetFlaggedStateArgs,
    MessageSetKeywordsMutationArgs, MessageSetReadStateArgs, MessageSetUserTagsArgs,
    MessageSnoozeArgs, MessageTargetArgs, MessageUnsnoozeArgs,
};
use crate::RevCursorArgs;

/// A parsed operation understood by every tier of the link. The message arms are
/// the live named-mutation vocabulary; `RevCursor` is the `revCursor` control
/// operation (D22) that used to be routed by a string compare outside the enum.
///
/// Adjacently tagged so the wire is `{"name": "message.setKeywords", "args":
/// {..}}` — the catalogued name is the serde tag.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "name", content = "args")]
pub enum MailOperation {
    #[serde(rename = "message.setKeywords")]
    SetKeywords(MessageSetKeywordsMutationArgs),
    #[serde(rename = "message.setReadState")]
    SetReadState(MessageSetReadStateArgs),
    #[serde(rename = "message.setFlaggedState")]
    SetFlaggedState(MessageSetFlaggedStateArgs),
    #[serde(rename = "message.setUserTags")]
    SetUserTags(MessageSetUserTagsArgs),
    #[serde(rename = "message.moveToMailbox")]
    MoveToMailbox(MessageMoveToMailboxArgs),
    #[serde(rename = "message.moveToRole")]
    MoveToRole(MessageMoveToRoleArgs),
    #[serde(rename = "message.replaceMailboxes")]
    ReplaceMailboxes(MessageReplaceMailboxesArgs),
    /// Add the message to a single mailbox (membership delta). The typed form of
    /// the REST `add-to-mailbox` command; folds as an `ApplyDiff` add.
    #[serde(rename = "message.addToMailbox")]
    AddToMailbox(MessageMailboxMembershipArgs),
    /// Remove the message from a single mailbox (membership delta). The typed
    /// form of the REST `remove-from-mailbox` command; folds as an `ApplyDiff`
    /// remove.
    #[serde(rename = "message.removeFromMailbox")]
    RemoveFromMailbox(MessageMailboxMembershipArgs),
    #[serde(rename = "message.destroy")]
    Destroy(MessageTargetArgs),
    #[serde(rename = "message.applyDiff")]
    ApplyDiff(MessageApplyDiffArgs),
    #[serde(rename = "message.snooze")]
    Snooze(MessageSnoozeArgs),
    #[serde(rename = "message.unsnooze")]
    Unsnooze(MessageUnsnoozeArgs),
    /// `revCursor` — a control operation (undo/redo cursor assignment) that
    /// targets no message and folds to no local effect.
    #[serde(rename = "revCursor")]
    RevCursor(RevCursorArgs),
}

impl MailOperation {
    /// The canonical wire name of this operation — the serde tag value. Kept in
    /// sync with the `#[serde(rename = …)]` tags above; the single source of the
    /// receipt's echoed name (one fact, derived from the variant).
    pub fn name(&self) -> &'static str {
        match self {
            MailOperation::SetKeywords(_) => "message.setKeywords",
            MailOperation::SetReadState(_) => "message.setReadState",
            MailOperation::SetFlaggedState(_) => "message.setFlaggedState",
            MailOperation::SetUserTags(_) => "message.setUserTags",
            MailOperation::MoveToMailbox(_) => "message.moveToMailbox",
            MailOperation::MoveToRole(_) => "message.moveToRole",
            MailOperation::ReplaceMailboxes(_) => "message.replaceMailboxes",
            MailOperation::AddToMailbox(_) => "message.addToMailbox",
            MailOperation::RemoveFromMailbox(_) => "message.removeFromMailbox",
            MailOperation::Destroy(_) => "message.destroy",
            MailOperation::ApplyDiff(_) => "message.applyDiff",
            MailOperation::Snooze(_) => "message.snooze",
            MailOperation::Unsnooze(_) => "message.unsnooze",
            MailOperation::RevCursor(_) => "revCursor",
        }
    }

    /// Account that owns the operation's target (or, for `RevCursor`, the account
    /// whose cursor is moving).
    pub fn account_id(&self) -> &str {
        match self {
            MailOperation::SetKeywords(args) => &args.source_id,
            MailOperation::SetReadState(args) => &args.source_id,
            MailOperation::SetFlaggedState(args) => &args.source_id,
            MailOperation::SetUserTags(args) => &args.source_id,
            MailOperation::MoveToMailbox(args) => &args.source_id,
            MailOperation::MoveToRole(args) => &args.source_id,
            MailOperation::ReplaceMailboxes(args) => &args.source_id,
            MailOperation::AddToMailbox(args) => &args.source_id,
            MailOperation::RemoveFromMailbox(args) => &args.source_id,
            MailOperation::Destroy(args) => &args.source_id,
            MailOperation::ApplyDiff(args) => &args.source_id,
            MailOperation::Snooze(args) => &args.source_id,
            MailOperation::Unsnooze(args) => &args.source_id,
            MailOperation::RevCursor(args) => &args.account_id,
        }
    }

    /// Target message id, or `None` for control operations (`RevCursor`) that
    /// target no message.
    pub fn message_id(&self) -> Option<&str> {
        Some(match self {
            MailOperation::SetKeywords(args) => &args.message_id,
            MailOperation::SetReadState(args) => &args.message_id,
            MailOperation::SetFlaggedState(args) => &args.message_id,
            MailOperation::SetUserTags(args) => &args.message_id,
            MailOperation::MoveToMailbox(args) => &args.message_id,
            MailOperation::MoveToRole(args) => &args.message_id,
            MailOperation::ReplaceMailboxes(args) => &args.message_id,
            MailOperation::AddToMailbox(args) => &args.message_id,
            MailOperation::RemoveFromMailbox(args) => &args.message_id,
            MailOperation::Destroy(args) => &args.message_id,
            MailOperation::ApplyDiff(args) => &args.message_id,
            MailOperation::Snooze(args) => &args.message_id,
            MailOperation::Unsnooze(args) => &args.message_id,
            MailOperation::RevCursor(_) => return None,
        })
    }

    /// The pure projection of this operation into link-core's fold vocabulary
    /// (D34 (b)) — the single local-effect derivation both the client's
    /// optimistic fold (wasm) and the runtime near node's outbox fold consume,
    /// so the two can never drift. `None` for operations with no local effect
    /// the tier can form from the request alone: role moves without a role map
    /// (see [`fold_effect_with_roles`](Self::fold_effect_with_roles)) and the
    /// `RevCursor` control op.
    pub fn fold_effect(&self) -> Option<MessageAssertion> {
        self.fold_effect_with_roles(&HashMap::new())
    }

    /// Like [`fold_effect`](Self::fold_effect) but resolves role moves
    /// (`moveToRole`/`snooze`/`unsnooze`) to a concrete `ReplaceMailboxes` via
    /// the account's `role → mailbox-id` map. `None` for a role absent from the
    /// map — graceful degradation (no optimism; the row leaves only on provider
    /// confirm) when the mailbox list is not loaded yet.
    pub fn fold_effect_with_roles(
        &self,
        roles: &HashMap<String, String>,
    ) -> Option<MessageAssertion> {
        match self {
            MailOperation::SetKeywords(args) => Some(MessageAssertion::SetKeywords {
                add: args.command.add.clone(),
                remove: args.command.remove.clone(),
            }),
            MailOperation::SetReadState(args) => {
                let command = keyword_toggle("$seen", args.read);
                Some(MessageAssertion::SetKeywords {
                    add: command.add,
                    remove: command.remove,
                })
            }
            MailOperation::SetFlaggedState(args) => {
                let command = keyword_toggle("$flagged", args.flagged);
                Some(MessageAssertion::SetKeywords {
                    add: command.add,
                    remove: command.remove,
                })
            }
            MailOperation::SetUserTags(args) => Some(MessageAssertion::SetKeywords {
                add: args.add.clone(),
                remove: args.remove.clone(),
            }),
            MailOperation::MoveToMailbox(args) => Some(MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec![args.mailbox_id.clone()],
            }),
            MailOperation::ReplaceMailboxes(args) => Some(MessageAssertion::ReplaceMailboxes {
                mailbox_ids: args.mailbox_ids.clone(),
            }),
            // Membership deltas fold as an ApplyDiff over the mailbox facet.
            MailOperation::AddToMailbox(args) => Some(MessageAssertion::ApplyDiff {
                diff: MessageChangeDiff {
                    mailboxes: posthaste_link_core::KeywordDelta {
                        added: vec![args.mailbox_id.clone()],
                        removed: Vec::new(),
                    },
                    ..Default::default()
                },
            }),
            MailOperation::RemoveFromMailbox(args) => Some(MessageAssertion::ApplyDiff {
                diff: MessageChangeDiff {
                    mailboxes: posthaste_link_core::KeywordDelta {
                        added: Vec::new(),
                        removed: vec![args.mailbox_id.clone()],
                    },
                    ..Default::default()
                },
            }),
            MailOperation::Destroy(_) => Some(MessageAssertion::Destroy),
            MailOperation::ApplyDiff(args) => Some(MessageAssertion::ApplyDiff {
                diff: args.diff.clone(),
            }),
            // Role moves resolve to ReplaceMailboxes via the account's role→id map.
            MailOperation::MoveToRole(args) => role_to_replace(roles, &args.role),
            // Snooze/unsnooze are role moves (to the snooze / inbox mailbox).
            MailOperation::Snooze(_) => role_to_replace(roles, "snooze"),
            MailOperation::Unsnooze(_) => role_to_replace(roles, "inbox"),
            // Control op: no message effect.
            MailOperation::RevCursor(_) => None,
        }
    }
}

/// Hand-written OpenAPI schema: the operation flattens into [`MutationRequest`]
/// as the top-level `name` (string) + `args` (open object) pair, matching the
/// wire the TS client builds. A typed `oneOf` over every arg struct would drag
/// `utoipa::ToSchema` through link-core (a wasm-pure frontier crate) for no
/// client gain — the client constructs these payloads by hand, not from the
/// generated schema.
#[cfg(feature = "openapi")]
impl utoipa::PartialSchema for MailOperation {
    fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
        use utoipa::openapi::schema::{ObjectBuilder, Type};
        ObjectBuilder::new()
            .property(
                "name",
                ObjectBuilder::new().schema_type(Type::String).build(),
            )
            .required("name")
            .property("args", ObjectBuilder::new().build())
            .into()
    }
}

#[cfg(feature = "openapi")]
impl utoipa::ToSchema for MailOperation {}

/// Resolve a `role` (e.g. "archive") to a `ReplaceMailboxes([mailbox_id])`
/// assertion via the account's role→mailbox-id map. `None` when the role is
/// absent (mailbox list not loaded) — the caller then falls back to no optimism.
fn role_to_replace(roles: &HashMap<String, String>, role: &str) -> Option<MessageAssertion> {
    roles
        .get(role)
        .map(|mailbox_id| MessageAssertion::ReplaceMailboxes {
            mailbox_ids: vec![mailbox_id.clone()],
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn adjacently_tagged_round_trip_uses_the_catalogued_name() {
        let value = json!({
            "name": "message.setKeywords",
            "args": {
                "sourceId": "acct",
                "messageId": "m1",
                "command": { "add": ["$seen"], "remove": [] }
            }
        });
        let op: MailOperation = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(op.name(), "message.setKeywords");
        assert_eq!(op.account_id(), "acct");
        assert_eq!(op.message_id(), Some("m1"));
        assert_eq!(serde_json::to_value(&op).unwrap(), value);
    }

    #[test]
    fn rev_cursor_is_a_control_op_with_no_message_target_or_effect() {
        let value = json!({
            "name": "revCursor",
            "args": { "accountId": "acct", "cursorStepId": null, "redoTail": [] }
        });
        let op: MailOperation = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(op.name(), "revCursor");
        assert_eq!(op.account_id(), "acct");
        assert_eq!(op.message_id(), None);
        assert_eq!(op.fold_effect(), None);
        assert_eq!(serde_json::to_value(&op).unwrap(), value);
    }

    #[test]
    fn role_move_without_a_map_entry_folds_to_none() {
        let op = MailOperation::MoveToRole(MessageMoveToRoleArgs {
            source_id: "acct".into(),
            message_id: "m1".into(),
            role: "archive".into(),
        });
        assert_eq!(op.fold_effect(), None);
        let mut roles = HashMap::new();
        roles.insert("archive".to_string(), "mbx-a".to_string());
        assert_eq!(
            op.fold_effect_with_roles(&roles),
            Some(MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec!["mbx-a".into()]
            })
        );
    }

    #[test]
    fn add_to_mailbox_folds_as_a_mailbox_add_delta() {
        let op = MailOperation::AddToMailbox(MessageMailboxMembershipArgs {
            source_id: "acct".into(),
            message_id: "m1".into(),
            mailbox_id: "mbx-a".into(),
        });
        match op.fold_effect().unwrap() {
            MessageAssertion::ApplyDiff { diff } => {
                assert_eq!(diff.mailboxes.added, vec!["mbx-a".to_string()]);
                assert!(diff.mailboxes.removed.is_empty());
            }
            other => panic!("expected ApplyDiff, got {other:?}"),
        }
    }
}
