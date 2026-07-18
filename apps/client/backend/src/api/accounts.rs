//! The accounts family: the accounts list with runtime health, one
//! account's full configuration (secrets redacted), identity/transport/logo
//! writes, the provider verification probe, deletion, and the OAuth flow.

use std::path::PathBuf;

use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use posthaste_client_models::{
    AccountRow, AccountSecretChange, AccountSettingsQuery, AccountSettingsResult,
    AccountTransportView, AccountsResult, CompleteOauthIntent, CreateAccountIntent,
    DeleteAccountIntent, OauthStartQuery, OauthStartResult, SetAccountLogoIntent,
    SetAccountSecretIntent, UpdateAccountIntent, UpdateAccountTransportIntent, VerifyAccountQuery,
    VerifyAccountResult,
};
use posthaste_domain_model::{
    AccountAppearance, AccountId, AccountSettings, AccountTransportSettings, DomainEvent, Id,
    SecretKind, SecretRef, SecretStatus, ServiceError, EVENT_TOPIC_ACCOUNT_CREATED,
    EVENT_TOPIC_ACCOUNT_DELETED, EVENT_TOPIC_ACCOUNT_UPDATED,
};

use super::{now_rfc3339, oauth, offload_read, ApiFailure, ApiState};
use crate::gateway::build_connection;
use crate::AppState;

/// Upper bound on an uploaded account logo (decoded bytes).
const MAX_ACCOUNT_LOGO_BYTES: usize = 2 * 1024 * 1024;

/// Accepted logo image types and the file extension each is stored under.
const ACCOUNT_LOGO_MIME_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("webp", "image/webp"),
    ("gif", "image/gif"),
];

pub(crate) async fn evaluate_accounts(app: &AppState) -> Result<AccountsResult, ApiFailure> {
    let service = app.service.clone();
    let (sources, default_account_id) = offload_read(move || {
        let sources = service.list_sources()?;
        let default_account_id = service.get_app_settings()?.default_account_id;
        Ok((sources, default_account_id))
    })
    .await?;
    let overviews = app.supervisor.runtime_overviews().await;
    let rows = sources
        .into_iter()
        .map(|source| {
            let overview = overviews
                .get(source.id.as_str())
                .cloned()
                .unwrap_or_default();
            AccountRow {
                is_default: default_account_id.as_ref() == Some(&source.id),
                id: source.id,
                name: source.name,
                full_name: source.full_name,
                enabled: source.enabled,
                status: overview.status,
                push: overview.push,
                last_sync_at: overview.last_sync_at,
                last_sync_error: overview.last_sync_error,
            }
        })
        .collect();
    Ok(AccountsResult { rows })
}

pub(crate) fn evaluate_account_settings(
    app: &AppState,
    query: AccountSettingsQuery,
) -> Result<AccountSettingsResult, ApiFailure> {
    let settings = load_account(app, &query.account_id)?;
    Ok(AccountSettingsResult {
        transport: transport_view(&settings.transport),
        id: settings.id,
        name: settings.name,
        full_name: settings.full_name,
        signature: settings.signature,
        email_patterns: settings.email_patterns,
        driver: settings.driver,
        enabled: settings.enabled,
        appearance: settings.appearance,
        created_at: settings.created_at,
        updated_at: settings.updated_at,
    })
}

/// The verification probe: connect to the provider once with the stored
/// transport and credential, fetch the identity where the provider exposes
/// one, and drop the connection. Nothing is stored; a connection failure is
/// the query's failure, carrying the error envelope the editor displays.
pub(crate) async fn evaluate_verify_account(
    app: &AppState,
    query: VerifyAccountQuery,
) -> Result<VerifyAccountResult, ApiFailure> {
    let settings = load_account(app, &query.account_id)?;
    let connection = build_connection(&settings, &app.secret_store, &app.store).await?;
    let identity_email = connection
        .gateway
        .fetch_identity(&settings.id)
        .await
        .ok()
        .map(|identity| identity.email);
    Ok(VerifyAccountResult {
        ok: true,
        identity_email,
        // The live probe's answer, not the driver table's: an IMAP account
        // with IDLE support counts as push-capable.
        push_supported: !connection.push_unsupported,
    })
}

pub(crate) fn evaluate_oauth_start(
    _app: &AppState,
    query: OauthStartQuery,
) -> Result<OauthStartResult, ApiFailure> {
    oauth::start_flow(&query)
}

pub(crate) async fn create_account(
    app: &AppState,
    intent: CreateAccountIntent,
) -> Result<u64, ApiFailure> {
    let name = intent.name.trim().to_string();
    if name.is_empty() {
        return Err(ApiFailure::malformed("account name must not be empty"));
    }
    let now = now_rfc3339();
    let settings = AccountSettings {
        id: AccountId::from(Id::generate()),
        name,
        full_name: intent.full_name,
        signature: intent.signature,
        email_patterns: intent.email_patterns,
        driver: posthaste_domain_model::AccountDriver::ImapSmtp,
        // A new account starts disabled unless asked otherwise: its
        // connection details are configured through the settings surface,
        // and an enabled account without them would only report errors.
        enabled: intent.enabled.unwrap_or(false),
        appearance: None,
        transport: AccountTransportSettings::default(),
        created_at: now.clone(),
        updated_at: now,
    };
    insert_source(app, &settings).await?;
    app.supervisor.start_account(&settings).await;
    Ok(publish_account_event(
        app,
        &settings.id,
        EVENT_TOPIC_ACCOUNT_CREATED,
    ))
}

pub(crate) async fn update_account(
    app: &AppState,
    intent: UpdateAccountIntent,
) -> Result<u64, ApiFailure> {
    let mut settings = load_account_offloaded(app, intent.account_id.clone()).await?;
    if let Some(name) = intent.name {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(ApiFailure::malformed("account name must not be empty"));
        }
        settings.name = name;
    }
    intent.full_name.apply(&mut settings.full_name);
    intent.signature.apply(&mut settings.signature);
    if let Some(email_patterns) = intent.email_patterns {
        settings.email_patterns = email_patterns;
    }
    if let Some(enabled) = intent.enabled {
        settings.enabled = enabled;
    }
    if let Some(appearance) = intent.appearance {
        settings.appearance = Some(appearance);
    }
    settings.updated_at = now_rfc3339();
    save_source(app, &settings).await?;
    // Restart (or park) the runtime under the new settings.
    app.supervisor.start_account(&settings).await;
    Ok(publish_account_event(
        app,
        &settings.id,
        EVENT_TOPIC_ACCOUNT_UPDATED,
    ))
}

/// Patch the transport endpoints. The intent has no field a credential could
/// ride in, so `secret_ref` is untouched by construction; the secret itself
/// moves only through `setAccountSecret`.
pub(crate) async fn update_account_transport(
    app: &AppState,
    intent: UpdateAccountTransportIntent,
) -> Result<u64, ApiFailure> {
    let mut settings = load_account_offloaded(app, intent.account_id.clone()).await?;
    if let Some(provider) = intent.provider {
        settings.transport.provider = provider;
    }
    if let Some(auth) = intent.auth {
        settings.transport.auth = auth;
    }
    intent.base_url.apply(&mut settings.transport.base_url);
    intent.username.apply(&mut settings.transport.username);
    if let Some(imap) = intent.imap {
        settings.transport.imap = Some(imap);
    }
    if let Some(smtp) = intent.smtp {
        settings.transport.smtp = Some(smtp);
    }
    settings.updated_at = now_rfc3339();
    save_source(app, &settings).await?;
    // Reconnect under the new endpoints.
    app.supervisor.start_account(&settings).await;
    Ok(publish_account_event(
        app,
        &settings.id,
        EVENT_TOPIC_ACCOUNT_UPDATED,
    ))
}

/// The one place secret material arrives. The material goes straight to the
/// secret store — it is never logged, never echoed back, and never lands in
/// the config document (which holds only the redacted reference).
pub(crate) async fn set_account_secret(
    app: &AppState,
    intent: SetAccountSecretIntent,
) -> Result<u64, ApiFailure> {
    // A form round-trip placeholder: nothing to store, nothing to restart.
    if matches!(intent.change, AccountSecretChange::Keep) {
        return Ok(app.events.generation());
    }
    if let AccountSecretChange::Replace { secret } = &intent.change {
        if secret.trim().is_empty() {
            return Err(ApiFailure::malformed("secret material must not be empty"));
        }
    }
    // The keyring write/delete and the config save are synchronous blocking
    // IPC + filesystem I/O; run the whole load-modify-store off the async
    // worker so a slow keyring daemon cannot stall it.
    let app_cloned = app.clone();
    let updated_at = now_rfc3339();
    let settings = offload_read(move || {
        let mut settings = load_account(&app_cloned, &intent.account_id)?;
        match intent.change {
            AccountSecretChange::Keep => unreachable!("handled above"),
            AccountSecretChange::Replace { secret } => {
                let material = secret.trim();
                // Reuse an existing OS-keyring reference; an env-var reference
                // is read-only from here, so a replace moves the account onto
                // its own keyring entry.
                let secret_ref = settings
                    .transport
                    .secret_ref
                    .clone()
                    .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
                    .unwrap_or_else(|| os_secret_ref(&settings.id));
                app_cloned
                    .secret_store
                    .save(&secret_ref, material)
                    .map_err(ServiceError::from)?;
                settings.transport.secret_ref = Some(secret_ref);
            }
            AccountSecretChange::Clear => {
                if let Some(existing) = settings.transport.secret_ref.take() {
                    if matches!(existing.kind, SecretKind::Os) {
                        app_cloned
                            .secret_store
                            .delete(&existing)
                            .map_err(ServiceError::from)?;
                    }
                }
            }
        }
        settings.updated_at = updated_at;
        app_cloned.service.save_source(&settings)?;
        Ok(settings)
    })
    .await?;
    // Reconnect under the new credential.
    app.supervisor.start_account(&settings).await;
    Ok(publish_account_event(
        app,
        &settings.id,
        EVENT_TOPIC_ACCOUNT_UPDATED,
    ))
}

/// Delete the account whole: stop its runtime, drop the stored credential
/// and logo, then remove the configuration and every row of its synced mail
/// data. The keyring and logo removals are best-effort — a keychain hiccup
/// must not strand the configuration.
pub(crate) async fn delete_account(
    app: &AppState,
    intent: DeleteAccountIntent,
) -> Result<u64, ApiFailure> {
    let settings = load_account_offloaded(app, intent.account_id.clone()).await?;
    app.supervisor.remove_account(&settings.id).await;
    // The keyring delete, logo removal, and config delete are synchronous
    // blocking IPC + filesystem I/O; run them off the async worker.
    let app_cloned = app.clone();
    let settings = offload_read(move || {
        if let Some(secret_ref) = settings
            .transport
            .secret_ref
            .as_ref()
            .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
        {
            if let Err(error) = app_cloned.secret_store.delete(secret_ref) {
                tracing::warn!(account_id = %settings.id, %error, "failed to delete account secret");
            }
        }
        if let Some(image_id) = appearance_image_id(&settings) {
            remove_logo_files(&app_cloned, &image_id);
        }
        app_cloned.service.delete_source(&settings.id)?;
        Ok(settings)
    })
    .await?;
    Ok(publish_account_event(
        app,
        &settings.id,
        EVENT_TOPIC_ACCOUNT_DELETED,
    ))
}

/// Store an uploaded logo under the config root and point the account's
/// appearance at it. The image travels base64 in the command body, the same
/// convention as compose attachments.
pub(crate) async fn set_account_logo(
    app: &AppState,
    intent: SetAccountLogoIntent,
) -> Result<u64, ApiFailure> {
    let mut settings = load_account_offloaded(app, intent.account_id.clone()).await?;
    let extension = logo_extension(&intent.mime_type)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(intent.content_base64.as_bytes())
        .map_err(|error| ApiFailure::malformed(format!("invalid base64 logo content: {error}")))?;
    if bytes.is_empty() {
        return Err(ApiFailure::malformed("account logo must not be empty"));
    }
    if bytes.len() > MAX_ACCOUNT_LOGO_BYTES {
        return Err(ApiFailure::malformed("account logo is too large"));
    }

    let image_id = Id::generate().to_string();
    let path = logo_root(app).join(format!("{image_id}.{extension}"));
    let write_path = path.clone();
    offload_read(move || {
        let parent = write_path.parent().expect("logo path has a parent");
        std::fs::create_dir_all(parent)
            .and_then(|()| std::fs::write(&write_path, &bytes))
            .map_err(|error| ApiFailure::internal(format!("failed to store account logo: {error}")))
    })
    .await?;

    let previous_image_id = appearance_image_id(&settings);
    let (initials, color_hue) = appearance_fallback_parts(&settings);
    settings.appearance = Some(AccountAppearance::Image {
        image_id: image_id.clone(),
        initials,
        color_hue,
    });
    settings.updated_at = now_rfc3339();
    let service = app.service.clone();
    let save_settings = settings.clone();
    offload_read(move || {
        if let Err(error) = service.save_source(&save_settings) {
            let _ = std::fs::remove_file(&path);
            return Err(error.into());
        }
        Ok(())
    })
    .await?;
    if let Some(previous_image_id) = previous_image_id {
        if previous_image_id != image_id {
            remove_logo_files(app, &previous_image_id);
        }
    }
    Ok(publish_account_event(
        app,
        &settings.id,
        EVENT_TOPIC_ACCOUNT_UPDATED,
    ))
}

/// Finish an authorization the `oauthStart` query began: exchange the code,
/// verify the identity, store the token set as the account secret, and
/// create the account on the provider's default mail endpoints.
pub(crate) async fn complete_oauth(
    app: &AppState,
    intent: CompleteOauthIntent,
) -> Result<u64, ApiFailure> {
    let seed = oauth::complete_flow(&intent.state, &intent.code).await?;
    let account_id = AccountId::from(Id::generate());
    let secret_ref = os_secret_ref(&account_id);
    // The credential lands in the keyring BEFORE the account exists in
    // config, so an enabled account is never observable without its secret.
    // The keyring save is synchronous blocking IPC — run it off the async
    // worker.
    {
        let secret_store = app.secret_store.clone();
        let secret_ref = secret_ref.clone();
        let token_set_json = seed.token_set_json.clone();
        offload_read(move || {
            secret_store
                .save(&secret_ref, &token_set_json)
                .map_err(ServiceError::from)?;
            Ok(())
        })
        .await?;
    }
    let now = now_rfc3339();
    let settings = AccountSettings {
        id: account_id,
        name: seed.identity_email.clone(),
        full_name: None,
        signature: None,
        email_patterns: vec![seed.identity_email.clone()],
        driver: posthaste_domain_model::AccountDriver::ImapSmtp,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings {
            provider: seed.provider,
            auth: posthaste_domain_model::ProviderAuthKind::OAuth2,
            base_url: None,
            username: Some(seed.identity_email),
            secret_ref: Some(secret_ref),
            imap: Some(seed.imap),
            smtp: Some(seed.smtp),
        },
        created_at: now.clone(),
        updated_at: now,
    };
    insert_source(app, &settings).await?;
    app.supervisor.start_account(&settings).await;
    Ok(publish_account_event(
        app,
        &settings.id,
        EVENT_TOPIC_ACCOUNT_CREATED,
    ))
}

/// `GET /account-assets/logos/{image_id}`: serve an uploaded account logo.
pub(crate) async fn handle_logo_download(
    State(state): State<ApiState>,
    Path(image_id): Path<String>,
) -> Result<Response, ApiFailure> {
    validate_logo_image_id(&image_id)?;
    let root = logo_root(&state.app);
    let found = offload_read(move || {
        for (extension, content_type) in ACCOUNT_LOGO_MIME_TYPES {
            let path = root.join(format!("{image_id}.{extension}"));
            match std::fs::read(&path) {
                Ok(bytes) => return Ok(Some((bytes, *content_type))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(ApiFailure::internal(format!(
                        "failed to read account logo: {error}"
                    )))
                }
            }
        }
        Ok(None)
    })
    .await?;
    let Some((bytes, content_type)) = found else {
        return Err(ApiFailure::unknown_id("account logo"));
    };
    Ok((
        [
            (header::CONTENT_TYPE, content_type.to_string()),
            (header::CACHE_CONTROL, "private, max-age=86400".to_string()),
        ],
        bytes,
    )
        .into_response())
}

/// Load one account's configuration or fail with the unknown-id envelope.
/// Synchronous config-store read — call from an already-offloaded context;
/// the async handlers use [`load_account_offloaded`].
fn load_account(app: &AppState, account_id: &AccountId) -> Result<AccountSettings, ApiFailure> {
    app.service
        .get_source(account_id)?
        .ok_or_else(|| ApiFailure::unknown_id(format!("account {}", account_id.as_str())))
}

/// [`load_account`] off the async worker: the config store is synchronous
/// filesystem I/O and must not block a Tokio worker thread.
async fn load_account_offloaded(
    app: &AppState,
    account_id: AccountId,
) -> Result<AccountSettings, ApiFailure> {
    let app = app.clone();
    offload_read(move || load_account(&app, &account_id)).await
}

/// Persist a new account document off the async worker (synchronous config
/// filesystem write).
async fn insert_source(app: &AppState, settings: &AccountSettings) -> Result<(), ApiFailure> {
    let service = app.service.clone();
    let settings = settings.clone();
    offload_read(move || Ok(service.insert_source(&settings)?)).await
}

/// Persist an existing account document off the async worker (synchronous
/// config filesystem write).
async fn save_source(app: &AppState, settings: &AccountSettings) -> Result<(), ApiFailure> {
    let service = app.service.clone();
    let settings = settings.clone();
    offload_read(move || Ok(service.save_source(&settings)?)).await
}

/// The secrets-safe transport projection: the stored credential surfaces
/// only as a redacted status. An OS-keyring reference hides its lookup key;
/// an env reference exposes the variable name as the label.
fn transport_view(transport: &AccountTransportSettings) -> AccountTransportView {
    AccountTransportView {
        provider: transport.provider.clone(),
        auth: transport.auth.clone(),
        base_url: transport.base_url.clone(),
        username: transport.username.clone(),
        imap: transport.imap.clone(),
        smtp: transport.smtp.clone(),
        secret: secret_status(transport.secret_ref.as_ref()),
    }
}

fn secret_status(secret_ref: Option<&SecretRef>) -> SecretStatus {
    match secret_ref {
        Some(secret_ref) => SecretStatus {
            storage: secret_ref.kind.clone(),
            configured: true,
            label: match secret_ref.kind {
                SecretKind::Env => Some(secret_ref.key.clone()),
                SecretKind::Os => None,
            },
        },
        None => SecretStatus {
            storage: SecretKind::Os,
            configured: false,
            label: None,
        },
    }
}

/// The default OS-keyring reference for an account's credential.
fn os_secret_ref(account_id: &AccountId) -> SecretRef {
    SecretRef {
        kind: SecretKind::Os,
        key: format!("account:{}", account_id.as_str()),
    }
}

/// Where uploaded account logos live, under the config root next to the
/// account documents they belong to.
fn logo_root(app: &AppState) -> PathBuf {
    app.paths.config_root.join("account-assets").join("logos")
}

fn logo_extension(mime_type: &str) -> Result<&'static str, ApiFailure> {
    let mime_type = mime_type.split(';').next().unwrap_or("").trim();
    ACCOUNT_LOGO_MIME_TYPES
        .iter()
        .find(|(_, candidate)| *candidate == mime_type)
        .map(|(extension, _)| *extension)
        .ok_or_else(|| {
            ApiFailure::malformed("account logo must be a PNG, JPEG, WebP, or GIF image")
        })
}

/// Logo image ids are backend-minted; anything else is rejected before it
/// can reach the filesystem as a path fragment.
fn validate_logo_image_id(image_id: &str) -> Result<(), ApiFailure> {
    let valid = !image_id.is_empty()
        && image_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
    if valid {
        Ok(())
    } else {
        Err(ApiFailure::malformed("account logo image id is invalid"))
    }
}

fn appearance_image_id(settings: &AccountSettings) -> Option<String> {
    match &settings.appearance {
        Some(AccountAppearance::Image { image_id, .. }) => Some(image_id.clone()),
        _ => None,
    }
}

/// Initials + hue to keep when the appearance flips to an image: the
/// existing appearance's parts, or name-derived initials and an id-derived
/// hue for an account that never had one.
fn appearance_fallback_parts(settings: &AccountSettings) -> (String, u16) {
    match &settings.appearance {
        Some(
            AccountAppearance::Initials {
                initials,
                color_hue,
            }
            | AccountAppearance::Image {
                initials,
                color_hue,
                ..
            },
        ) => (initials.clone(), *color_hue),
        None => {
            let initials: String = settings
                .name
                .split_whitespace()
                .filter_map(|word| word.chars().next())
                .take(2)
                .flat_map(char::to_uppercase)
                .collect();
            let initials = if initials.is_empty() {
                "?".to_string()
            } else {
                initials
            };
            let color_hue = settings.id.as_str().bytes().fold(0u32, |acc, byte| {
                acc.wrapping_mul(31).wrapping_add(u32::from(byte))
            }) % 360;
            (initials, color_hue as u16)
        }
    }
}

/// Best-effort removal of a stored logo image across the accepted
/// extensions; a leftover file is cosmetic, never load-bearing.
fn remove_logo_files(app: &AppState, image_id: &str) {
    if validate_logo_image_id(image_id).is_err() {
        return;
    }
    let root = logo_root(app);
    for (extension, _) in ACCOUNT_LOGO_MIME_TYPES {
        let path = root.join(format!("{image_id}.{extension}"));
        match std::fs::remove_file(&path) {
            Ok(()) => return,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(image_id, %error, "failed to remove account logo file");
                return;
            }
        }
    }
}

/// Publish an account-configuration event so every connected client observes
/// the change on the stream (bumping the generation), and return the
/// resulting generation.
fn publish_account_event(app: &AppState, account_id: &AccountId, topic: &str) -> u64 {
    app.events.publish(&[DomainEvent {
        seq: 0,
        account_id: account_id.clone(),
        topic: topic.to_string(),
        occurred_at: now_rfc3339(),
        mailbox_id: None,
        message_id: None,
        payload: serde_json::Value::Null,
    }]);
    app.events.generation()
}
