//! Unified message-mutation dispatch table.
//!
//! Both the runtime near node and the authority far node must recognize the same
//! set of named `message.*` mutations. The old code parsed the mutation name and
//! deserialized the args separately in three places (runtime build, runtime
//! near-node assertion fold, authority backend). This module centralizes that
//! name-to-args mapping and the cross-cutting metadata each node needs.
//!
//! - Near node: scope enforcement, diff eligibility, optimistic assertion.
//! - Far node: backend command execution (kept in `authority-runtime/backend.rs`
//!   because it uses internal `Backend` methods).

use posthaste_link_core::MessageAssertion;
use posthaste_runtime_contract::mutation_args::{
    keyword_toggle, MessageApplyDiffArgs, MessageMoveToMailboxArgs, MessageMoveToRoleArgs,
    MessageReplaceMailboxesArgs, MessageSetFlaggedStateArgs, MessageSetKeywordsMutationArgs,
    MessageSetReadStateArgs, MessageSetUserTagsArgs, MessageTargetArgs,
};
use posthaste_runtime_contract::{MutationRequest, RuntimeError};

/// A parsed message mutation understood by both the runtime near node and the
/// authority far node.
#[derive(Debug)]
pub enum MessageMutation {
    SetKeywords(MessageSetKeywordsMutationArgs),
    SetReadState(MessageSetReadStateArgs),
    SetFlaggedState(MessageSetFlaggedStateArgs),
    SetUserTags(MessageSetUserTagsArgs),
    MoveToMailbox(MessageMoveToMailboxArgs),
    MoveToRole(MessageMoveToRoleArgs),
    ReplaceMailboxes(MessageReplaceMailboxesArgs),
    Archive(MessageTargetArgs),
    Trash(MessageTargetArgs),
    RestoreToInbox(MessageTargetArgs),
    Destroy(MessageTargetArgs),
    ApplyDiff(MessageApplyDiffArgs),
}

impl MessageMutation {
    /// Parse and dispatch a `MutationRequest` by its `name`.
    pub fn from_request(request: &MutationRequest) -> Result<Self, RuntimeError> {
        Ok(match request.name.as_str() {
            "message.setKeywords" => MessageMutation::SetKeywords(parse_args(request)?),
            "message.setReadState" => MessageMutation::SetReadState(parse_args(request)?),
            "message.setFlaggedState" => MessageMutation::SetFlaggedState(parse_args(request)?),
            "message.setUserTags" => MessageMutation::SetUserTags(parse_args(request)?),
            "message.moveToMailbox" => MessageMutation::MoveToMailbox(parse_args(request)?),
            "message.moveToRole" => MessageMutation::MoveToRole(parse_args(request)?),
            "message.replaceMailboxes" => MessageMutation::ReplaceMailboxes(parse_args(request)?),
            "message.archive" => MessageMutation::Archive(parse_args(request)?),
            "message.trash" => MessageMutation::Trash(parse_args(request)?),
            "message.restoreToInbox" => MessageMutation::RestoreToInbox(parse_args(request)?),
            "message.destroy" => MessageMutation::Destroy(parse_args(request)?),
            "message.applyDiff" => MessageMutation::ApplyDiff(parse_args(request)?),
            _ => {
                return Err(RuntimeError::invalid_mutation(format!(
                    "unknown runtime mutation '{}'",
                    request.name
                )));
            }
        })
    }

    /// Account that owns the targeted message.
    pub fn account_id(&self) -> &str {
        match self {
            MessageMutation::SetKeywords(args) => &args.source_id,
            MessageMutation::SetReadState(args) => &args.source_id,
            MessageMutation::SetFlaggedState(args) => &args.source_id,
            MessageMutation::SetUserTags(args) => &args.source_id,
            MessageMutation::MoveToMailbox(args) => &args.source_id,
            MessageMutation::MoveToRole(args) => &args.source_id,
            MessageMutation::ReplaceMailboxes(args) => &args.source_id,
            MessageMutation::Archive(args) => &args.source_id,
            MessageMutation::Trash(args) => &args.source_id,
            MessageMutation::RestoreToInbox(args) => &args.source_id,
            MessageMutation::Destroy(args) => &args.source_id,
            MessageMutation::ApplyDiff(args) => &args.source_id,
        }
    }

    /// Target message id.
    pub fn message_id(&self) -> &str {
        match self {
            MessageMutation::SetKeywords(args) => &args.message_id,
            MessageMutation::SetReadState(args) => &args.message_id,
            MessageMutation::SetFlaggedState(args) => &args.message_id,
            MessageMutation::SetUserTags(args) => &args.message_id,
            MessageMutation::MoveToMailbox(args) => &args.message_id,
            MessageMutation::MoveToRole(args) => &args.message_id,
            MessageMutation::ReplaceMailboxes(args) => &args.message_id,
            MessageMutation::Archive(args) => &args.message_id,
            MessageMutation::Trash(args) => &args.message_id,
            MessageMutation::RestoreToInbox(args) => &args.message_id,
            MessageMutation::Destroy(args) => &args.message_id,
            MessageMutation::ApplyDiff(args) => &args.message_id,
        }
    }

    /// Whether a successful mutation should capture and record an invertible
    /// change-diff for undo/redo.
    pub fn diff_eligible(&self) -> bool {
        // Destroy is non-invertible; applyDiff is the undo/redo vehicle itself
        // and never records a fresh diff.
        !matches!(
            self,
            MessageMutation::Destroy(_) | MessageMutation::ApplyDiff(_)
        )
    }

    /// Optimistic assertion the runtime near node can fold into a mail-list
    /// view before the backend confirms. `None` for mutations whose effect
    /// cannot be resolved locally (role moves) or is non-invertible (destroy).
    pub fn to_assertion(&self) -> Option<MessageAssertion> {
        match self {
            MessageMutation::SetKeywords(args) => Some(MessageAssertion::SetKeywords {
                add: args.command.add.clone(),
                remove: args.command.remove.clone(),
            }),
            MessageMutation::SetReadState(args) => {
                let command = keyword_toggle("$seen", args.read);
                Some(MessageAssertion::SetKeywords {
                    add: command.add,
                    remove: command.remove,
                })
            }
            MessageMutation::SetFlaggedState(args) => {
                let command = keyword_toggle("$flagged", args.flagged);
                Some(MessageAssertion::SetKeywords {
                    add: command.add,
                    remove: command.remove,
                })
            }
            MessageMutation::SetUserTags(args) => Some(MessageAssertion::SetKeywords {
                add: args.add.clone(),
                remove: args.remove.clone(),
            }),
            MessageMutation::MoveToMailbox(args) => Some(MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec![args.mailbox_id.clone()],
            }),
            MessageMutation::ReplaceMailboxes(args) => Some(MessageAssertion::ReplaceMailboxes {
                mailbox_ids: args.mailbox_ids.clone(),
            }),
            MessageMutation::Destroy(_) => Some(MessageAssertion::Destroy),
            MessageMutation::ApplyDiff(args) => Some(MessageAssertion::ApplyDiff {
                diff: args.diff.clone(),
            }),
            // Role moves need account role→mailbox resolution, so they are not
            // folded optimistically yet.
            MessageMutation::MoveToRole(_)
            | MessageMutation::Archive(_)
            | MessageMutation::Trash(_)
            | MessageMutation::RestoreToInbox(_) => None,
        }
    }
}

fn parse_args<T>(request: &MutationRequest) -> Result<T, RuntimeError>
where
    T: for<'de> serde::Deserialize<'de>,
{
    posthaste_runtime_contract::mutation_args::parse_args(request)
}
