//! The runtime's link **far-end** (RFC D37/D39): the serving half a node
//! mounts only because it fans in downstream links — here, the runtime
//! serving its clients.
//!
//! Per the node anatomy (topology §2.1b), serving is an adapter, not a
//! replica half: links (per-link frame routing, settlement-to-originator,
//! frame collapse/delta) and the view registry's frame emission / event pump
//! live here, beside each other. The view *projection* parts (recompute,
//! windowing, coverage — [`crate::views`]) stay wire-agnostic: they must not
//! know whether their views render directly or get framed/linked/paginated.

pub(crate) mod links;
pub(crate) mod view_registry;
