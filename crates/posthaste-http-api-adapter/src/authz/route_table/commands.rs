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
    // Named-mutation funnel: this ONE route carries EVERY mutation op
    // (setKeywords … replaceMailboxes … destroy … send), so no single static
    // action can gate it — a static verb both under-gates (a tag-scoped token
    // could destroy or send) and over-blocks (a move-scoped token could not
    // archive). The action axis is handler-derived per operation instead
    // (deny-by-default, exhaustive over `MailOperation` — see
    // `authz::operation` and `api::runtime_stream::mutations`); the perimeter
    // still enforces the account query-filter and expiry caveats here.
    Entry {
        method: "POST",
        template: "/runtime/sessions/{session_id}/mutations",
        authz: filter_handler_derived_action(ResourceShape::account("sourceId")),
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
