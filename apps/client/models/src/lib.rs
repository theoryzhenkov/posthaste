//! Protocol models for the client: the state document, surfaces, patches,
//! and commands. The single source of truth for both ends — TypeScript types
//! are GENERATED from this crate (ts-rs) into `frontend/src/gen/`; nothing
//! protocol-shaped is hand-written twice.
//!
//! Dependency allowlist: `serde` + `ts-rs` + the domain model. The link/frame
//! vocabulary (contract-core) must never enter this crate.
//!
//! Intentionally empty: the document/surface/patch/command shapes are being
//! decided before implementation (see `apps/client/README.md`).
