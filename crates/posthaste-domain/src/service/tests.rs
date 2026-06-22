use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;

use crate::*;

mod config;
mod fixtures;
mod mutation_gateway;
mod store;
mod store_automation_impls;
mod store_command_event_impls;
mod store_read_impls;
mod store_sync_cache_impls;

use config::*;
use fixtures::*;
use mutation_gateway::*;
use store::*;

mod automation;
mod body_cache_budget;
mod body_cache_worker;
mod cache_rescore;
mod identity_fallback;
mod message_mutation_cursors;
mod message_mutation_retries;
mod outbox;
mod smart_mailboxes;
mod source_cleanup;
mod sync_cache_candidates;
