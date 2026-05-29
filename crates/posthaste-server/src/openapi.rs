//! OpenAPI document for the Posthaste REST surface.
//!
//! The Rust handlers are the single source of truth: each handler is annotated
//! with `#[utoipa::path]` and each wire type derives `ToSchema`. [`ApiDoc`]
//! aggregates them into one document, served at `GET /v1/openapi.json` and
//! emitted to the committed `openapi.json` contract artifact.
//!
//! @spec docs/L1-api#openapi-contract

use axum::Json;
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
        crate::api::list_accounts,
        crate::api::get_account,
        crate::api::create_account,
        crate::api::patch_account,
        crate::api::verify_account,
        crate::api::start_provider_oauth,
        crate::api::start_account_oauth,
        crate::api::complete_account_oauth,
        crate::api::enable_account,
        crate::api::disable_account,
        crate::api::upload_account_logo,
        crate::api::get_account_logo,
        crate::api::delete_account,
        crate::api::reload_config,
        crate::api::list_mailboxes,
        crate::api::patch_mailbox,
        crate::api::get_sidebar,
        crate::api::list_source_messages,
        crate::api::search_messages,
        crate::api::list_conversations,
        crate::api::get_conversation,
        crate::api::get_message,
        crate::api::get_message_attachment,
        crate::api::get_identity,
        crate::api::list_sender_addresses,
        crate::api::get_reply_context,
        crate::api::send_message,
        crate::api::trigger_sync,
        crate::api::stream_events,
        crate::api::message_commands::set_keywords,
        crate::api::message_commands::add_to_mailbox,
        crate::api::message_commands::remove_from_mailbox,
        crate::api::message_commands::replace_mailboxes,
        crate::api::message_commands::destroy_message,
        crate::api::smart_mailboxes::list_smart_mailboxes,
        crate::api::smart_mailboxes::create_smart_mailbox,
        crate::api::smart_mailboxes::get_smart_mailbox,
        crate::api::smart_mailboxes::patch_smart_mailbox,
        crate::api::smart_mailboxes::delete_smart_mailbox,
        crate::api::smart_mailboxes::reset_default_smart_mailboxes,
        crate::api::smart_mailboxes::list_smart_mailbox_messages,
        crate::api::smart_mailboxes::list_smart_mailbox_conversations,
        crate::api::settings::get_settings,
        crate::api::settings::patch_settings,
        crate::api::settings::preview_automation_rule,
    ),
    components(schemas(
        // Server-local wire types.
        crate::api::HealthResponse,
        crate::api::ApiErrorBody,
        crate::api::ApiErrorCode,
        crate::api::OkResponse,
        crate::api::VerificationResponse,
        crate::api::TriggerSyncRequest,
        crate::api::TriggerSyncResponse,
        crate::api::ConversationPageResponse,
        crate::api::MessagePageResponse,
        crate::api::AutomationRulePreviewResponse,
        crate::api::StartOAuthResponse,
        crate::api::CreateAccountRequest,
        crate::api::PatchAccountRequest,
        crate::api::StartOAuthRequest,
        crate::api::StartProviderOAuthRequest,
        crate::api::PatchMailboxRequest,
        crate::api::PatchSettingsRequest,
        crate::api::PreviewAutomationRuleRequest,
        crate::api::CreateSmartMailboxRequest,
        crate::api::PatchSmartMailboxRequest,
        crate::api::AccountTransportRequest,
        crate::api::SecretWriteRequest,
        crate::api::SecretWriteMode,
        // Domain wire types and their transitive closure.
        posthaste_domain::AccountId,
        posthaste_domain::MailboxId,
        posthaste_domain::MessageId,
        posthaste_domain::ThreadId,
        posthaste_domain::BlobId,
        posthaste_domain::ConversationId,
        posthaste_domain::SmartMailboxId,
        posthaste_domain::AccountOverview,
        posthaste_domain::AccountDriver,
        posthaste_domain::AccountAppearance,
        posthaste_domain::AccountConnectionOverview,
        posthaste_domain::AccountRuntimeOverview,
        posthaste_domain::AccountStatus,
        posthaste_domain::PushStatus,
        posthaste_domain::SyncProgress,
        posthaste_domain::SyncProgressStage,
        posthaste_domain::SyncTrigger,
        posthaste_domain::ProviderHint,
        posthaste_domain::ProviderKind,
        posthaste_domain::ProviderAuthKind,
        posthaste_domain::TransportSecurity,
        posthaste_domain::ImapTransportSettings,
        posthaste_domain::SmtpTransportSettings,
        posthaste_domain::SecretKind,
        posthaste_domain::SecretStatus,
        posthaste_domain::AppSettings,
        posthaste_domain::AppAppearanceSettings,
        posthaste_domain::AppThemeMode,
        posthaste_domain::AppPalettePreset,
        posthaste_domain::AppUiDensity,
        posthaste_domain::AppGlassThemeSettings,
        posthaste_domain::AppGlassBloomSettings,
        posthaste_domain::CachePolicy,
        posthaste_domain::AutomationRule,
        posthaste_domain::AutomationTrigger,
        posthaste_domain::AutomationAction,
        posthaste_domain::MailboxSummary,
        posthaste_domain::MessageSummary,
        posthaste_domain::MessageDetail,
        posthaste_domain::MessageAttachment,
        posthaste_domain::MessageSortField,
        posthaste_domain::RawMessageRef,
        posthaste_domain::Recipient,
        posthaste_domain::ConversationSummary,
        posthaste_domain::ConversationView,
        posthaste_domain::ConversationSortField,
        posthaste_domain::SortDirection,
        posthaste_domain::SourceMessageRef,
        posthaste_domain::SidebarResponse,
        posthaste_domain::SidebarSource,
        posthaste_domain::SidebarSmartMailbox,
        posthaste_domain::TagSummary,
        posthaste_domain::SmartMailbox,
        posthaste_domain::SmartMailboxSummary,
        posthaste_domain::SmartMailboxKind,
        posthaste_domain::SmartMailboxRule,
        posthaste_domain::SmartMailboxRuleNode,
        posthaste_domain::SmartMailboxGroup,
        posthaste_domain::SmartMailboxGroupOperator,
        posthaste_domain::SmartMailboxCondition,
        posthaste_domain::SmartMailboxField,
        posthaste_domain::SmartMailboxOperator,
        posthaste_domain::SmartMailboxValue,
        posthaste_domain::Identity,
        posthaste_domain::CachedSenderAddress,
        posthaste_domain::ReplyContext,
        posthaste_domain::CommandResult,
        posthaste_domain::SetKeywordsCommand,
        posthaste_domain::AddToMailboxCommand,
        posthaste_domain::RemoveFromMailboxCommand,
        posthaste_domain::ReplaceMailboxesCommand,
        posthaste_domain::SendMessageRequest,
        posthaste_domain::SyncMode,
        posthaste_domain::DomainEvent,
    )),
    tags(
        (name = "system", description = "Health and service status"),
        (name = "accounts", description = "Account configuration and lifecycle"),
        (name = "oauth", description = "Provider OAuth authorization flows"),
        (name = "mailboxes", description = "Mailboxes and navigation sidebar"),
        (name = "messages", description = "Messages, attachments, compose, and commands"),
        (name = "conversations", description = "Conversation list and detail views"),
        (name = "smart-mailboxes", description = "Saved-query smart mailboxes"),
        (name = "settings", description = "Application settings and automation rules"),
        (name = "sync", description = "Sync triggers and configuration reload"),
        (name = "events", description = "Server-sent domain event stream")
    )
)]
pub struct ApiDoc;

/// Generate the OpenAPI document. Single entry point for both the served route
/// and the committed-artifact contract test.
pub fn document() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

/// `GET /v1/openapi.json` — serve the generated OpenAPI document.
pub async fn openapi_json() -> Json<utoipa::openapi::OpenApi> {
    Json(document())
}
