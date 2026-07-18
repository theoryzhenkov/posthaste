//! Profiling and benchmarking harness for PostHaste's offline hot paths.
//!
//! The [`workloads`] module holds deterministic, network-free fixtures and the
//! operations exercised by the `posthaste-profile` binary, the Criterion timing
//! benches, and the iai-callgrind regression gate.

pub mod workloads;
