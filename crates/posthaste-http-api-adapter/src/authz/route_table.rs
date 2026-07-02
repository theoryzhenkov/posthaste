use std::collections::HashMap;
use std::sync::OnceLock;

use super::*;

mod accounts;
mod commands;
mod read_routes;
mod smart_mailboxes;

/// One entry of the static authz table. Listed as `(method, template, authz)`
/// so it reads as a reviewable security artifact and so the completeness test
/// can confirm every operationId is covered.
pub(crate) struct Entry {
    pub(crate) method: &'static str,
    pub(crate) template: &'static str,
    pub(crate) authz: RouteAuthz,
}

const fn gate(action: Action, resource: ResourceShape) -> RouteAuthz {
    RouteAuthz {
        action,
        resource,
        mode: ScopeMode::Gate,
    }
}

const fn filter(action: Action, resource: ResourceShape) -> RouteAuthz {
    RouteAuthz {
        action,
        resource,
        mode: ScopeMode::Filter,
    }
}

const AUTHZ_TABLES: &[&[Entry]] = &[
    accounts::ROUTES,
    smart_mailboxes::ROUTES,
    read_routes::ROUTES,
    commands::ROUTES,
];

fn authz_entries() -> impl Iterator<Item = &'static Entry> {
    AUTHZ_TABLES.iter().flat_map(|table| table.iter())
}

pub(crate) fn authz_entry_count() -> usize {
    AUTHZ_TABLES.iter().map(|table| table.len()).sum()
}

/// Build the method+template → `RouteAuthz` map once.
pub(crate) fn authz_map() -> &'static HashMap<String, RouteAuthz> {
    static MAP: OnceLock<HashMap<String, RouteAuthz>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = HashMap::with_capacity(authz_entry_count());
        for entry in authz_entries() {
            let key = route_key(entry.method, entry.template);
            debug_assert!(!map.contains_key(&key), "duplicate authz entry for {key}");
            map.insert(key, entry.authz);
        }
        map
    })
}

/// Look up the authz descriptor for a matched route (method + nest-stripped
/// template). `None` means the route is unmapped — the caller must fail CLOSED
/// (treat as misconfiguration, deny) so a new route cannot ship open.
pub fn lookup(method: &str, template: &str) -> Option<RouteAuthz> {
    authz_map().get(&route_key(method, template)).copied()
}

/// Every `(method, template)` pair in the authz table. Used by the completeness
/// test to confirm coverage against the OpenAPI document.
pub fn mapped_routes() -> Vec<(&'static str, &'static str)> {
    authz_entries()
        .map(|entry| (entry.method, entry.template))
        .collect()
}
