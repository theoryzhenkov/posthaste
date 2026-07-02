//! OpenAPI document for the Posthaste REST surface.
//!
//! The Rust handlers are the single source of truth: each handler is annotated
//! with `#[utoipa::path]` and each wire type derives `ToSchema`. [`ApiDoc`]
//! aggregates them into one document, served at `GET /v1/openapi.json` and
//! emitted to the committed `openapi.json` contract artifact.
//!
//! @spec docs/L1-api#openapi-contract

use axum::routing::get;
use axum::{Json, Router};
use utoipa::OpenApi;

/// Aggregated OpenAPI document for the `/v1` REST surface.
///
/// As handlers are annotated during P1, register each one under `paths(...)`
/// and each wire type under `components(schemas(...))`.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Posthaste API",
        version = "0.1.0",
        license(name = "MIT", identifier = "MIT"),
        description = "Local-first JMAP mail client backend. The `/v1` surface is the \
                       documented, versioned contract for first-party clients, custom \
                       clients, and agents."
    ),
    paths(
        crate::api::health,
        crate::api::read_calls::read,
        crate::api::accounts::crud::list_accounts,
        crate::api::accounts::crud::get_account,
        crate::api::accounts::crud::create_account,
        crate::api::accounts::crud::patch_account,
        crate::api::accounts::crud::verify_account,
        crate::api::auth_tokens::create_auth_token,
        crate::api::accounts::lifecycle::enable_account,
        crate::api::accounts::lifecycle::disable_account,
        crate::api::accounts::logos::upload_account_logo,
        crate::api::accounts::logos::get_account_logo,
        crate::api::accounts::crud::delete_account,
        crate::api::accounts::lifecycle::reload_config,
        crate::api::mailboxes::list_mailboxes,
        crate::api::mailboxes::patch_mailbox,
        crate::api::messages::listing::list_source_messages,
        crate::api::messages::listing::search_messages,
        crate::api::messages::listing::list_conversations,
        crate::api::messages::detail::handlers::get_conversation,
        crate::api::messages::detail::handlers::get_message,
        crate::api::messages::detail::handlers::get_message_attachment,
        crate::api::messages::detail::handlers::get_message_body,
        crate::api::messages::compose::reads::get_identity,
        crate::api::messages::compose::reads::list_sender_addresses,
        crate::api::messages::compose::reads::get_reply_context,
        crate::api::messages::compose::reads::get_draft_content,
        crate::api::messages::compose::drafts::send_message,
        crate::api::messages::compose::drafts::save_draft,
        crate::api::messages::compose::drafts::delete_draft,
        crate::api::messages::compose::operations::list_pending_operations,
        crate::api::messages::compose::operations::discard_operation,
        crate::api::messages::compose::operations::retry_operation,
        crate::api::sync_events::trigger_sync,
        crate::api::sync_events::stream_events,
        crate::api::views::open_view,
        crate::api::views::stream_view,
        crate::api::runtime_stream::links::open_runtime_link,
        crate::api::runtime_stream::links::close_runtime_link,
        crate::api::runtime_stream::links::stream_runtime_link,
        crate::api::runtime_stream::views::open_runtime_link_view,
        crate::api::runtime_stream::views::close_runtime_link_view,
        crate::api::runtime_stream::views::extend_runtime_link_view,
        crate::api::runtime_stream::mutations::run_runtime_link_mutation,
        crate::api::runtime_stream::mutations::runtime_link_mutation_settlement,
        crate::api::message_commands::set_keywords,
        crate::api::message_commands::add_to_mailbox,
        crate::api::message_commands::remove_from_mailbox,
        crate::api::message_commands::replace_mailboxes,
        crate::api::message_commands::destroy_message,
        crate::api::smart_mailboxes::crud::list_smart_mailboxes,
        crate::api::smart_mailboxes::crud::create_smart_mailbox,
        crate::api::smart_mailboxes::crud::get_smart_mailbox,
        crate::api::smart_mailboxes::crud::patch_smart_mailbox,
        crate::api::smart_mailboxes::crud::delete_smart_mailbox,
        crate::api::smart_mailboxes::crud::reset_default_smart_mailboxes,
        crate::api::smart_mailboxes::listings::list_smart_mailbox_messages,
        crate::api::smart_mailboxes::listings::list_smart_mailbox_conversations,
        crate::api::settings::get_settings,
        crate::api::settings::patch_settings,
        crate::api::settings::preview_automation_rule,
    ),
    components(schemas(
        // Server-local wire types.
        crate::api::HealthResponse,
        crate::api::ReadRequest,
        crate::api::ReadCall,
        crate::api::ReadOperation,
        crate::api::ReadCallArgs,
        crate::api::AccountIdSelector,
        crate::api::ReadResponse,
        crate::api::ReadResult,
        crate::api::AccountListReadResult,
        crate::api::MailboxListReadResult,
        crate::api::SmartMailboxListReadResult,
        crate::api::TagListReadResult,
        crate::api::ApiErrorBody,
        crate::api::ApiErrorCode,
        crate::api::OkResponse,
        crate::api::VerificationResponse,
        crate::api::TriggerSyncRequest,
        crate::api::TriggerSyncResponse,
        crate::api::views::OpenViewRequest,
        crate::api::views::OpenViewResponse,
        crate::api::runtime_stream::OpenRuntimeLinkViewRequest,
        crate::api::runtime_stream::OpenRuntimeLinkViewResponse,
        crate::api::runtime_stream::ExtendRuntimeLinkViewRequest,
        crate::api::ConversationPageResponse,
        crate::api::MessagePageResponse,
        crate::api::AutomationRulePreviewResponse,
        crate::api::CreateAccountRequest,
        crate::api::CreateAuthTokenRequest,
        crate::api::CreateAuthTokenResponse,
        crate::authz::Action,
        crate::api::PatchAccountRequest,
        crate::api::PatchMailboxRequest,
        crate::api::PatchSettingsRequest,
        crate::api::PreviewAutomationRuleRequest,
        crate::api::CreateSmartMailboxRequest,
        crate::api::PatchSmartMailboxRequest,
        crate::api::AccountTransportRequest,
        crate::api::SecretWriteRequest,
        crate::api::SecretWriteMode,
        // Domain wire types and their transitive closure.
        posthaste_domain_model::AccountId,
        posthaste_domain_model::MailboxId,
        posthaste_domain_model::MessageId,
        posthaste_domain_model::ThreadId,
        posthaste_domain_model::BlobId,
        posthaste_domain_model::ConversationId,
        posthaste_domain_model::SmartMailboxId,
        posthaste_domain_model::AccountOverview,
        posthaste_domain_model::AccountDriver,
        posthaste_domain_model::AccountAppearance,
        posthaste_domain_model::AccountConnectionOverview,
        posthaste_domain_model::AccountRuntimeOverview,
        posthaste_domain_model::AccountStatus,
        posthaste_domain_model::PushStatus,
        posthaste_domain_model::SyncProgress,
        posthaste_domain_model::SyncProgressStage,
        posthaste_domain_model::SyncTrigger,
        posthaste_domain_model::ProviderHint,
        posthaste_domain_model::ProviderKind,
        posthaste_domain_model::ProviderAuthKind,
        posthaste_domain_model::TransportSecurity,
        posthaste_domain_model::ImapTransportSettings,
        posthaste_domain_model::SmtpTransportSettings,
        posthaste_domain_model::SecretKind,
        posthaste_domain_model::SecretStatus,
        posthaste_domain_model::AppSettings,
        posthaste_domain_model::CachePolicy,
        posthaste_domain_model::AutomationRule,
        posthaste_domain_model::AutomationTrigger,
        posthaste_domain_model::AutomationAction,
        posthaste_domain_model::MailboxSummary,
        posthaste_domain_model::MessageSummary,
        posthaste_domain_model::MessageDetail,
        posthaste_domain_model::MessageAttachment,
        posthaste_domain_model::MessageSortField,
        posthaste_domain_model::RawMessageRef,
        posthaste_domain_model::Recipient,
        posthaste_domain_model::ConversationSummary,
        posthaste_domain_model::ConversationView,
        posthaste_domain_model::ConversationSortField,
        posthaste_domain_model::SortDirection,
        posthaste_domain_model::SourceMessageRef,
        posthaste_domain_model::TagSummary,
        posthaste_domain_model::SmartMailbox,
        posthaste_domain_model::SmartMailboxSummary,
        posthaste_domain_model::SmartMailboxKind,
        posthaste_domain_model::SmartMailboxRule,
        posthaste_domain_model::SmartMailboxRuleNode,
        posthaste_domain_model::SmartMailboxGroup,
        posthaste_domain_model::SmartMailboxGroupOperator,
        posthaste_domain_model::SmartMailboxCondition,
        posthaste_domain_model::SmartMailboxField,
        posthaste_domain_model::SmartMailboxOperator,
        posthaste_domain_model::SmartMailboxValue,
        posthaste_domain_model::Identity,
        posthaste_domain_model::CachedSenderAddress,
        posthaste_domain_model::ReplyContext,
        posthaste_domain_model::CommandResult,
        posthaste_domain_model::CommandAck,
        posthaste_domain_model::SetKeywordsCommand,
        posthaste_domain_model::AddToMailboxCommand,
        posthaste_domain_model::RemoveFromMailboxCommand,
        posthaste_domain_model::ReplaceMailboxesCommand,
        posthaste_domain_model::SendMessageAttachment,
        posthaste_domain_model::SendMessageRequest,
        crate::api::messages::SaveDraftRequest,
        crate::api::messages::DeleteDraftRequest,
        posthaste_domain_model::Operation,
        posthaste_domain_model::OperationEntity,
        posthaste_domain_model::OperationEntityKind,
        posthaste_domain_model::OperationKind,
        posthaste_domain_model::OperationState,
        posthaste_domain_model::OperationId,
        posthaste_domain_model::SyncMode,
        posthaste_domain_model::DomainEvent,
        posthaste_contract_core::RuntimeLinkConnection,
        posthaste_contract_core::RuntimeLinkId,
        posthaste_contract_core::RuntimeLinkSeq,
        posthaste_contract_core::RuntimeFrame,
        posthaste_contract_core::ClientMutationId,
        posthaste_contract_core::RuntimeMutationId,
        posthaste_contract_core::MutationRequest,
        posthaste_contract_core::MutationReceipt,
        posthaste_contract_core::RuntimeMutationSettlement,
        posthaste_contract_core::MutationSettlementState,
        posthaste_contract_core::MutationNotification,
        posthaste_contract_core::ViewId,
        posthaste_contract_core::ViewRevision,
        posthaste_contract_core::ViewSnapshot,
        posthaste_contract_core::ViewDescriptor,
        posthaste_contract_core::ViewLifecycle,
        posthaste_contract_core::RuntimeCoverage,
        posthaste_contract_core::CoverageRange,
        posthaste_contract_core::ReadWatermark,
        posthaste_contract_core::RuntimeAdapterError,
        posthaste_contract_core::RuntimeErrorCode,
    )),
    tags(
        (name = "system", description = "Health and service status"),
        (name = "read", description = "Typed batch read calls"),
        (name = "accounts", description = "Account configuration and lifecycle"),
        (name = "mailboxes", description = "Mailboxes and navigation sidebar"),
        (name = "messages", description = "Messages, attachments, compose, and commands"),
        (name = "conversations", description = "Conversation list and detail views"),
        (name = "smart-mailboxes", description = "Saved-query smart mailboxes"),
        (name = "settings", description = "Application settings and automation rules"),
        (name = "sync", description = "Sync triggers and configuration reload"),
        (name = "events", description = "Server-sent domain event stream"),
        (name = "views", description = "Runtime-owned view snapshots and streams"),
        (name = "runtime", description = "Link-scoped runtime frame stream"),
        (name = "auth", description = "Capability-token minting")
    )
)]
pub struct ApiDoc;

/// Generate the OpenAPI document. Single entry point for both the served route
/// and the committed-artifact contract test.
pub fn document() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

/// The public contract-document routes (`/openapi.json` + `/asyncapi.json`),
/// kept out of [`build_api_router`](crate::build_api_router) so each deployment
/// serves its OWN OpenAPI document: the lean near node serves
/// [`document`] (no OAuth), the bundled server serves the far document that adds
/// the OAuth-flow routes. Both routes are perimeter-exempt.
pub fn openapi_router(document: utoipa::openapi::OpenApi) -> Router {
    Router::new()
        .route(
            "/openapi.json",
            get(move || {
                let document = document.clone();
                async move { Json(document) }
            }),
        )
        .route("/asyncapi.json", get(asyncapi_json))
}

/// The committed AsyncAPI event contract, embedded at build time. This is the
/// event-driven analogue of `openapi.json`, describing the `/v1/events` SSE
/// stream and the full set of event topics.
///
/// @spec docs/L1-api#sse-event-stream
const ASYNCAPI_JSON: &str = include_str!("../../../asyncapi.json");

/// `GET /v1/asyncapi.json` — serve the committed AsyncAPI event contract.
pub async fn asyncapi_json() -> impl axum::response::IntoResponse {
    (
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        ASYNCAPI_JSON,
    )
}
