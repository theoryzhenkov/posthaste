//! Transport-neutral runtime contract shared by Posthaste runtime implementations.
//!
//! The types in this crate intentionally avoid Axum, Tauri, frontend, provider-client,
//! SQLite-table, or replica-table dependencies.
//!
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#runtime-contract-crate-first
//! spec: docs/eph/PLAN-L2-bundled-app-test-plan#contract-no-transport-types

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeSessionId(String);

impl RuntimeSessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ViewId(String);

impl ViewId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ViewRevision(u64);

impl ViewRevision {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubscriptionId(String);

impl SubscriptionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ClientMutationId(String);

impl ClientMutationId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeMutationId(String);

impl RuntimeMutationId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCaller {
    pub session_id: Option<RuntimeSessionId>,
    pub capabilities: RuntimeCallerCapabilities,
    pub account_scope: Option<Vec<String>>,
    pub operation_source: RuntimeOperationSource,
    pub correlation_id: Option<String>,
}

impl RuntimeCaller {
    pub fn system() -> Self {
        Self {
            session_id: None,
            capabilities: RuntimeCallerCapabilities::default(),
            account_scope: None,
            operation_source: RuntimeOperationSource::System,
            correlation_id: None,
        }
    }

    pub fn test() -> Self {
        Self {
            operation_source: RuntimeOperationSource::Test,
            ..Self::system()
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCallerCapabilities {
    #[serde(default)]
    pub actions: Vec<RuntimeCapability>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeCapability {
    Read,
    Manage,
    Send,
    Tag,
    Move,
    Delete,
    Resource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeOperationSource {
    System,
    Api,
    Desktop,
    Renderer,
    Test,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStatus {
    pub lifecycle: RuntimeLifecycle,
    pub store: RuntimeStoreStatus,
    pub account_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeLifecycle {
    Starting,
    Ready,
    Degraded,
    Stopping,
    Stopped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeStoreStatus {
    pub config_loaded: bool,
    pub state_store_open: bool,
    pub cache_root_ready: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewDescriptor {
    pub family: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewLifecycle {
    Loading,
    Ready,
    Updating,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadWatermark {
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCoverage {
    pub kind: RuntimeCoverageKind,
    #[serde(default)]
    pub details: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeCoverageKind {
    Complete,
    Partial,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewSnapshot {
    pub view_id: ViewId,
    pub descriptor: ViewDescriptor,
    pub revision: ViewRevision,
    pub lifecycle: ViewLifecycle,
    pub read_watermark: Option<ReadWatermark>,
    pub coverage: RuntimeCoverage,
    #[serde(default)]
    pub data: Value,
    #[serde(default)]
    pub pending_mutations: Vec<RuntimeMutationId>,
    pub error: Option<RuntimeAdapterError>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationRequest {
    pub session_id: Option<RuntimeSessionId>,
    pub name: String,
    #[serde(default)]
    pub args: Value,
    pub client_mutation_id: ClientMutationId,
    pub context: Option<Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MutationReceipt {
    pub runtime_mutation_id: Option<RuntimeMutationId>,
    pub client_mutation_id: ClientMutationId,
    pub state: MutationSettlementState,
    pub error: Option<RuntimeAdapterError>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MutationSettlementState {
    Accepted,
    LocalApplied,
    Queued,
    Confirmed,
    Failed,
    Conflict,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAdapterError {
    pub code: RuntimeErrorCode,
    pub message: String,
    pub retryable: bool,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub details: Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeErrorCode {
    RuntimeNotReady,
    InvalidDescriptor,
    InvalidMutation,
    Unauthorized,
    NotFound,
    ProviderUnavailable,
    Conflict,
    TransportDisconnected,
    Internal,
}

#[derive(Debug)]
pub struct RuntimeError(pub RuntimeAdapterError);

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}: {}", self.0.code, self.0.message)
    }
}

impl std::error::Error for RuntimeError {}

impl RuntimeError {
    pub fn internal(message: impl Into<String>, correlation_id: Option<String>) -> Self {
        Self(RuntimeAdapterError {
            code: RuntimeErrorCode::Internal,
            message: message.into(),
            retryable: false,
            correlation_id,
            details: Value::Null,
        })
    }

    pub fn envelope(&self) -> &RuntimeAdapterError {
        &self.0
    }
}

#[async_trait]
pub trait RuntimeCore: Send + Sync {
    async fn runtime_status(&self, caller: RuntimeCaller) -> Result<RuntimeStatus, RuntimeError>;
}
