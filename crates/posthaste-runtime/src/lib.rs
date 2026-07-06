//! The authority runtime near node.
//!
//! Builds a transport-free authority runtime handle that implements the shared
//! runtime surface extracted from `RuntimeCore` ([`posthaste_runtime_api`] +
//! [`posthaste_client_link`]) without binding HTTP or creating desktop windows.
//! It reaches its authority server over the [`posthaste_authority_server_link`] link (the
//! in-process `LocalAuthorityServer` lives in the far-node crate; this crate ships the
//! remote [`RemoteAuthorityServer`]), so it never links the far-node roles
//! (store/engine/imap). The far-node crate composes a runtime over an in-process
//! authority server via [`assemble_runtime`].

mod apply_ledger;
mod assembly;
mod far_end;
mod handle;
mod link_near_end;
mod near_node;
mod read;
mod secret;
mod shutdown;
mod transport;
mod views;

pub use apply_ledger::{DurableApplyRecord, DurableApplyState, DurableApplyStore, DurableReserve};
pub use assembly::{
    assemble_runtime, build_remote_runtime, AuthorityServerTransportConfig,
    AuthorityServerTransportDecorator, ComposedRuntime, RemoteRuntimeBuild, RuntimeAssembly,
    RuntimeBuildConfig,
};
pub use handle::RuntimeHandle;
pub use read::ReadCache;
pub use secret::SystemSecretStore;
pub use shutdown::{RuntimeBuildError, RuntimeShutdownError, RuntimeShutdownHandle};
pub use transport::RemoteAuthorityServer;
