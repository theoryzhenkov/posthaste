//! The authority runtime near node.
//!
//! Builds a transport-free authority runtime handle that implements the shared
//! runtime contract ([`posthaste_runtime_contract`]) without binding HTTP or
//! creating desktop windows. It reaches its backend over the
//! [`posthaste_link_contract`] link (the in-process `LocalBackend` lives in the
//! far-node crate; this crate ships the remote [`RemoteBackend`]), so it never
//! links the far-node roles (store/engine/imap). The far-node crate composes a
//! runtime over an in-process backend via [`assemble_runtime`].

mod build;
pub mod mutation_args;
mod near_node;
mod read;
mod secret;
mod sessions;
mod transport;
mod views;

pub use build::{
    assemble_runtime, build_remote_runtime, AuthorityRuntimeBuildConfig,
    AuthorityRuntimeBuildError, AuthorityRuntimeHandle, AuthorityRuntimeShutdownError,
    BackendTransportConfig, BackendTransportDecorator, ComposedRuntime, RemoteRuntimeBuild,
    RuntimeAssembly, RuntimeShutdownHandle,
};
pub use read::ReadCache;
pub use secret::SystemSecretStore;
pub use transport::RemoteBackend;
