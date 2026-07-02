//! wasm-bindgen boundary for the client-layer reactive entity store.
//!
//! Exposes the portable [`posthaste_link_replica`] entity store to JavaScript
//! so the web `entityStoreAdapter` ([client-link L2 §6](../replication/client-link/L2.md),
//! slice 2e) can drive it in the browser. The host (JS) owns transport
//! (fetch/SSE to the remote runtime) and persistence (IndexedDB); this boundary
//! is pure compute over values passed as JSON strings, which keeps the
//! dependency surface to `wasm-bindgen` alone (no `serde-wasm-bindgen`) and the
//! type contract explicit.
//!
//! @spec docs/replication/client-link/L2#3-the-wasm-boundary-posthaste-link-wasm
//! @spec docs/eph/DESIGN-L2-client-link-reactive-store (2e)

pub mod entity_store;
pub mod mutation;

// The near-end engine boundary (RFC-L2-architecture-cleanup D41): a NEW async
// boundary shape beside the sync `EntityStoreHandle` — JS hands an IO object at
// construction and the handle exposes Promise-returning lifecycle methods. It
// binds `wasm-bindgen-futures`/`js-sys`/`gloo-timers`, so it compiles only for
// the browser target; the native `cargo check --workspace` build skips it.
#[cfg(target_arch = "wasm32")]
pub mod near_end;
