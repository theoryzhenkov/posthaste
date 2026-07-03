//! Authority runtime implementation over local Posthaste state.
//!
//! This crate is the first extraction seam out of `posthaste-server`: it builds
//! a transport-free authority runtime handle that implements the shared runtime
//! contract without binding HTTP or creating desktop windows.
//!
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#runtime-contract-crate-first
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#authority-runtime-handle-test-first

mod account_reads;
mod account_repository;
mod authority_server;
mod bootstrap;
mod build;
mod live_accounts;
mod link_wire;
mod local_authority_server;
mod mail_queries;
mod mutations;
pub mod oauth;
mod push;
pub mod rules;
mod runtime_registry;
pub mod supervisor;
#[cfg(test)]
mod test_support;

// The far-node crate's own assembly surface.
pub use account_reads::AccountRuntimeOverviewProvider;
pub use build::{
    build_authority_server, build_authority_server_node, from_api_bridge_for_migration,
    from_api_bridge_with_account_supervisor_for_migration, AuthorityServerApiMigrationBridge,
    AuthorityServerBuild, AuthorityServerNode, MigrationRuntime,
};
pub use link_wire::{link_router, LinkAuth};
pub use live_accounts::{LiveAccountRuntimeProvider, UnavailableLiveAccountRuntimeProvider};
pub use mutations::AccountMutationService;
pub use rules::{
    load_rules, CapabilityMinter, ManagedRulesHandle, RuleConfigError, RuleEngineHandle,
    RuleTokenGrant, RuleWriteError, SharedMinter,
};
pub use supervisor::AccountSupervisor;
