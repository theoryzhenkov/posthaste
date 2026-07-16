//! The client's backend: the one evaluator.
//!
//! Will own, once the shapes are decided (see `apps/client/README.md`):
//! - sessions + the per-session versioned state document
//! - the surface materializer over the store's `_effective` views
//! - the dirty → coalesce → diff recomputer
//! - the SSE document stream + snapshot refetch (recovery == connect)
//! - the command endpoint (typed mail intents → the outbox)
//!
//! Intentionally empty: scaffolding only, implementation shape under design.
