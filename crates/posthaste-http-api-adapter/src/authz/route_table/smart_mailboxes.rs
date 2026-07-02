use super::*;

pub(super) const ROUTES: &[Entry] = &[
    // -- Smart mailboxes: definitions are global config (Manage to mutate,
    //    Read to view). Their message/conversation LISTS are Filter aggregates. --
    Entry {
        method: "GET",
        template: "/smart-mailboxes",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/smart-mailboxes",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "GET",
        template: "/smart-mailboxes/{smart_mailbox_id}",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "PATCH",
        template: "/smart-mailboxes/{smart_mailbox_id}",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "DELETE",
        template: "/smart-mailboxes/{smart_mailbox_id}",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/smart-mailboxes:reset-defaults",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    // Smart-mailbox MESSAGE list: no `sourceId` query param exists, so it stays a
    // global read (an account caveat is unsatisfiable → such tokens denied).
    // SECURITY: do not add a query axis without first adding + enforcing a source
    // filter param in the handler.
    Entry {
        method: "GET",
        template: "/smart-mailboxes/{smart_mailbox_id}/messages",
        authz: filter(Action::Read, ResourceShape::empty()),
    },
    // Smart-mailbox CONVERSATION list: result-side scoped on `sourceId`. The
    // handler ANDs a `source_message_scope_rule` into the smart-mailbox rule in
    // BOTH branches (Tier-1 result-side scoping), so an `account=X` token with a
    // matching `?sourceId=X` sees only that account; a mismatched/absent source
    // makes the caveat unsatisfiable → 403. `mailbox` is intentionally NOT a
    // satisfier here (mailbox ids are not account-unique).
    Entry {
        method: "GET",
        template: "/smart-mailboxes/{smart_mailbox_id}/conversations",
        authz: filter(Action::Read, ResourceShape::account("sourceId")),
    },
];
