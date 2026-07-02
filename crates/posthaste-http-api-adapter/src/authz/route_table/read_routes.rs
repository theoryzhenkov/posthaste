use super::*;

pub(super) const ROUTES: &[Entry] = &[
    // -- Conversation views. The list is result-side scoped on `sourceId`: the
    //    handler ANDs a `source_message_scope_rule` into the query in BOTH the
    //    search and non-search branches (Tier-1 result-side scoping), so an
    //    `account=X` token with a matching `?sourceId=X` sees only that account;
    //    a mismatched/absent source makes the caveat unsatisfiable → 403.
    //    `mailbox` is intentionally NOT a satisfier (mailbox ids are not
    //    account-unique). A single conversation is addressed by an opaque
    //    conversation id (no scopable account axis in the path), so it is a global
    //    Gate read. SECURITY: keep the handler's source scope in every branch. --
    Entry {
        method: "GET",
        template: "/views/conversations",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    Entry {
        method: "GET",
        template: "/views/conversations/{conversation_id}",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/views",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    Entry {
        method: "GET",
        template: "/views/{view_id}/stream",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    Entry {
        method: "POST",
        template: "/runtime/sessions",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    Entry {
        method: "DELETE",
        template: "/runtime/sessions/{session_id}",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    Entry {
        method: "GET",
        template: "/runtime/sessions/{session_id}/stream",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    // The settlement query is a READ of the session's mutation ledger (the
    // reconciler's D44b probe) — it changes nothing, so it gates on Read even
    // though the POST beside it gates on Tag.
    Entry {
        method: "GET",
        template: "/runtime/sessions/{session_id}/mutations/{client_mutation_id}",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    Entry {
        method: "POST",
        template: "/runtime/sessions/{session_id}/views",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    Entry {
        method: "DELETE",
        template: "/runtime/sessions/{session_id}/views/{view_id}",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    Entry {
        method: "POST",
        template: "/runtime/sessions/{session_id}/views/{view_id}/extend",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
    // -- Per-source resources: account axis from `source_id`. --
    Entry {
        method: "GET",
        template: "/sources/{source_id}/mailboxes",
        authz: gate(Action::Read, ResourceShape::account("source_id")),
    },
    Entry {
        method: "PATCH",
        template: "/sources/{source_id}/mailboxes/{mailbox_id}",
        authz: gate(
            Action::Manage,
            ResourceShape::account_mailbox("source_id", "mailbox_id"),
        ),
    },
    // Per-source message list: the source is in the path (Gate on account);
    // `mailboxId` is an optional query filter (not a path resource), so this is
    // a Gate, not a Filter — the account axis is exact from the path.
    Entry {
        method: "GET",
        template: "/sources/{source_id}/messages",
        authz: gate(Action::Read, ResourceShape::account("source_id")),
    },
    // Search is a cross-account aggregate with NO source filter param → global
    // read; an account-scoped token cannot be satisfied (no `sourceId` to match).
    Entry {
        method: "GET",
        template: "/messages/search",
        authz: filter(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/messages/{message_id}",
        authz: gate(
            Action::Read,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/messages/{message_id}/attachments/{attachment_id}",
        authz: gate(
            Action::Read,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/messages/{message_id}/body",
        authz: gate(
            Action::Read,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "GET",
        template: "/sender-addresses",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    // SSE domain-event stream (GET /v1/events): a cross-account *read* feed for
    //    view-less consumers (posthastectl's `events` tap). It accepts `accountId`
    //    /`mailboxId` query filters, so it is a Filter aggregate keyed on those
    //    params — a matching `accountId` satisfies an account-scoped caveat, a
    //    missing/non-matching one denies. A pure read (no mutation), so it belongs
    //    in the read table, not commands.
    Entry {
        method: "GET",
        template: "/events",
        authz: filter(
            Action::Read,
            ResourceShape::account_mailbox("accountId", "mailboxId"),
        ),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/identity",
        authz: gate(Action::Read, ResourceShape::account("source_id")),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/messages/{message_id}/reply-context",
        authz: gate(
            Action::Read,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
    Entry {
        method: "GET",
        template: "/sources/{source_id}/messages/{message_id}/draft-content",
        authz: gate(
            Action::Read,
            ResourceShape::account_message("source_id", "message_id"),
        ),
    },
];
