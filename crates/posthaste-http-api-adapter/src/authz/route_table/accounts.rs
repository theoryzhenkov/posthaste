use super::*;

pub(super) const ROUTES: &[Entry] = &[
    // -- Account / settings / config: management surface, no resource axis the
    //    caveat model can scope (you cannot scope "list all accounts" to one
    //    account), so a scoped token is correctly rejected on these. --
    Entry {
        method: "GET",
        template: "/settings",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "PATCH",
        template: "/settings",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/automation-rules:preview",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    // The read-only automation-rules list (config-file-only rules; no write
    // path). No resource axis a scoped token can narrow — like `/settings`.
    Entry {
        method: "GET",
        template: "/rules",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "GET",
        template: "/accounts",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/accounts",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    // Token mint: derives a capability token. `Mint` + no resource axis means
    // only a full-scope (or unscoped `action = ...,mint,...`) caller may reach
    // the handler — a resource-scoped caller is rejected here regardless of
    // action, since `ResourceShape::empty` cannot satisfy an account/mailbox/
    // message caveat. Unlike every other verb, `mint` is an ISSUANCE right, not
    // a substantive scope: `derive_capability_token` mints FRESH from the root
    // key for a caller that holds `mint` (rather than attenuating the caller's
    // own token), so a `mint`-only caller — e.g. the discovery bootstrap
    // ({mint, tap:read}, RFC-L2-scripting §7 ruling 11) — can obtain tokens
    // WIDER than its own scope. A caller withOUT `mint` (e.g. a plain
    // `action = manage` token) still only ever narrows, same as before.
    Entry {
        method: "POST",
        template: "/auth/tokens",
        authz: gate(Action::Mint, ResourceShape::empty()),
    },
    // Single-account routes: account axis from the `account_id` path param.
    Entry {
        method: "GET",
        template: "/accounts/{account_id}",
        authz: gate(Action::Read, ResourceShape::account("account_id")),
    },
    Entry {
        method: "PATCH",
        template: "/accounts/{account_id}",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "DELETE",
        template: "/accounts/{account_id}",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "POST",
        template: "/accounts/{account_id}/verify",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    // The OAuth provider-flow routes are the far half of `/v1` (served by
    // `posthaste-server`, not the lean near platform), but they remain in the
    // authz table because the bundled server documents them in `openapi.json` and
    // the completeness check requires openapi↔authz parity. `/oauth/callback` is
    // perimeter-exempt (the provider redirect carries no bearer token), so it has
    // no entry.
    Entry {
        method: "POST",
        template: "/accounts/{account_id}/oauth/start",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "POST",
        template: "/oauth/start",
        authz: gate(Action::Manage, ResourceShape::empty()),
    },
    Entry {
        method: "POST",
        template: "/accounts/{account_id}/enable",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "POST",
        template: "/accounts/{account_id}/disable",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    Entry {
        method: "POST",
        template: "/accounts/{account_id}/logo",
        authz: gate(Action::Manage, ResourceShape::account("account_id")),
    },
    // Logo asset is keyed by an opaque image id, not an account/message — a
    // read with no scopable resource axis.
    Entry {
        method: "GET",
        template: "/account-assets/logos/{image_id}",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
    // -- Typed read calls can be cross-account reads; an account-scoped token
    //    cannot be satisfied here in the initial global-gate implementation. --
    Entry {
        method: "POST",
        template: "/read",
        authz: gate(Action::Read, ResourceShape::empty()),
    },
];
