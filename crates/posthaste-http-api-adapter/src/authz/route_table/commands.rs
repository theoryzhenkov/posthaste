use super::*;

pub(super) const ROUTES: &[Entry] = &[
    // -- Commands: write verbs scoped to the source (and message where present). --
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/send",
        authz: gate(Action::Send, ResourceShape::account("source_id")),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/save-draft",
        authz: gate(Action::Send, ResourceShape::account("source_id")),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/delete-draft",
        authz: gate(Action::Send, ResourceShape::account("source_id")),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/operations",
        authz: gate(Action::Read, ResourceShape::account("source_id")),
    },
    // Outbox recovery: discarding or retrying a pending operation manages the
    // source's outbox (mirrors the source-scoped `commands/sync` manage gate).
    Entry {
        method: "DELETE",
        template: "/sources/{source_id}/operations/{operation_id}",
        authz: gate(Action::Manage, ResourceShape::account("source_id")),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/operations/{operation_id}/retry",
        authz: gate(Action::Manage, ResourceShape::account("source_id")),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/messages/{message_id}/set-keywords",
        authz: gate(
            Action::Tag,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "POST",
        template: "/runtime/sessions/{session_id}/mutations",
        authz: filter(Action::Tag, ResourceShape::account("sourceId")),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/messages/{message_id}/add-to-mailbox",
        authz: gate(
            Action::Move,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/messages/{message_id}/remove-from-mailbox",
        authz: gate(
            Action::Move,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/messages/{message_id}/replace-mailboxes",
        authz: gate(
            Action::Move,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/messages/{message_id}/destroy",
        authz: gate(
            Action::Delete,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "POST",
        template: "/sources/{source_id}/commands/sync",
        authz: gate(Action::Manage, ResourceShape::account("source_id")),
    },
    Entry {
        method: "POST",
        template: "/config:reload",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
];
