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

use std::collections::HashMap;

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
            MessageMutation::Destroy(args) => &args.message_id,
            MessageMutation::ApplyDiff(args) => &args.message_id,
        }
    }

    /// Optimistic assertion the runtime near node can fold into a mail-list
    /// view before the backend confirms. `None` for role moves when no role map
    /// is supplied (the legacy no-optimism path) or when the role is absent
    /// from the map (mailbox list not loaded yet); `Destroy` is non-invertible
    /// but still folded (the row leaves immediately).
    pub fn to_assertion(&self) -> Option<MessageAssertion> {
        self.to_assertion_with_roles(&HashMap::new())
    }

    /// Like [`to_assertion`](Self::to_assertion) but resolves role moves
    /// (`archive`/`trash`/`restoreToInbox`/`moveToRole`) to `ReplaceMailboxes`
    /// via the account's role→mailbox-id `roles` map. `None` for a role not in
    /// the map — graceful degradation (no optimism; the row leaves only on
    /// provider confirm) when the mailbox list isn't loaded yet. Non-role moves
    /// ignore the map. This is fix (b) for the move/archive flicker: the role
    /// move now carries optimism, so fix (a)'s equal-version hold can keep it
    /// folded through the unconfirmed window.
    pub fn to_assertion_with_roles(
        &self,
        roles: &HashMap<String, String>,
    ) -> Option<MessageAssertion> {
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
            // Role moves resolve to ReplaceMailboxes via the account's role→id map.
            MessageMutation::MoveToRole(args) => role_to_replace(roles, &args.role),
        }
    }
}

/// Resolve a `role` (e.g. "archive") to a `ReplaceMailboxes([mailbox_id])`
/// assertion via the account's role→mailbox-id map. `None` when the role is
/// absent (mailbox list not loaded) — the caller then falls back to no optimism.
fn role_to_replace(roles: &HashMap<String, String>, role: &str) -> Option<MessageAssertion> {
    roles.get(role).map(|mailbox_id| MessageAssertion::ReplaceMailboxes {
        mailbox_ids: vec![mailbox_id.clone()],
    })
}

fn parse_args<T>(request: &MutationRequest) -> Result<T, RuntimeError>
where
    T: for<'de> serde::Deserialize<'de>,
{
    posthaste_runtime_contract::mutation_args::parse_args(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn role_map() -> HashMap<String, String> {
        let mut m = HashMap::new();
        m.insert("inbox".into(), "mbx-inbox".into());
        m.insert("archive".into(), "mbx-archive".into());
        m.insert("trash".into(), "mbx-trash".into());
        m
    }

    fn parse(name: &str, args: serde_json::Value) -> MessageMutation {
        let request: MutationRequest = serde_json::from_value(serde_json::json!({
            "name": name,
            "args": args,
            "clientMutationId": "op"
        }))
        .unwrap();
        MessageMutation::from_request(&request).unwrap()
    }

    #[test]
    fn role_moves_resolve_to_replace_mailboxes_via_the_role_map() {
        let map = role_map();
        // A role move (moveToRole) carries the role explicitly — archive/trash/
        // restoreToInbox are no longer separate mutations (they were 1:1 aliases
        // of moveToRole with a hardcoded role).
        let archive = parse(
            "message.moveToRole",
            serde_json::json!({ "sourceId": "acct", "messageId": "m1", "role": "archive" }),
        )
        .to_assertion_with_roles(&map);
        assert_eq!(
            archive,
            Some(MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec!["mbx-archive".into()]
            })
        );

        let restore = parse(
            "message.moveToRole",
            serde_json::json!({ "sourceId": "acct", "messageId": "m1", "role": "inbox" }),
        )
        .to_assertion_with_roles(&map);
        assert_eq!(
            restore,
            Some(MessageAssertion::ReplaceMailboxes {
                mailbox_ids: vec!["mbx-inbox".into()]
            })
        );
    }

    #[test]
    fn role_move_without_a_map_entry_returns_none() {
        // Graceful degradation: the mailbox list isn't loaded yet → no optimism,
        // no regression (the row leaves only on provider confirm).
        let map = HashMap::new(); // no roles
        let archive = parse(
            "message.moveToRole",
            serde_json::json!({ "sourceId": "acct", "messageId": "m1", "role": "archive" }),
        );
        assert_eq!(archive.to_assertion_with_roles(&map), None);
        // And the legacy no-map path still returns None for role moves.
        assert_eq!(archive.to_assertion(), None);
    }

    #[test]
    fn non_role_moves_ignore_the_role_map() {
        let map = role_map();
        let set_keywords = parse(
            "message.setKeywords",
            serde_json::json!({
                "sourceId": "acct",
                "messageId": "m1",
                "command": { "add": ["$seen"], "remove": [] }
            }),
        );
        assert_eq!(
            set_keywords.to_assertion_with_roles(&map),
            set_keywords.to_assertion()
        );
    }
}
