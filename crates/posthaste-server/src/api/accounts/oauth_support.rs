use super::*;

pub(crate) fn oauth_success_html() -> Html<String> {
    Html(
        "<!doctype html><meta charset=\"utf-8\"><title>Posthaste OAuth</title><p>Authentication complete. You can return to Posthaste.</p>".to_string(),
    )
}

pub(crate) fn oauth_processing_html() -> Html<String> {
    Html(
        "<!doctype html><meta charset=\"utf-8\"><meta http-equiv=\"refresh\" content=\"1\"><title>Posthaste OAuth</title><p>Authentication is still completing. This page will refresh automatically.</p>".to_string(),
    )
}

pub(crate) fn validate_oauth_start_request<'a>(
    client_id: &'a str,
    client_secret: Option<&'a str>,
    redirect_uri: &'a str,
) -> Result<(&'a str, Option<&'a str>, &'a str), ApiError> {
    let client_id = client_id.trim();
    if client_id.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidOauthRequest,
            "clientId is required",
        ));
    }
    let redirect_uri = redirect_uri.trim();
    if redirect_uri.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            ApiErrorCode::InvalidOauthRequest,
            "redirectUri is required",
        ));
    }
    Ok((
        client_id,
        client_secret
            .map(str::trim)
            .filter(|client_secret| !client_secret.is_empty()),
        redirect_uri,
    ))
}

pub(crate) async fn create_oauth_account_from_exchange(
    state: &Arc<AppState>,
    profile: &OAuthProviderProfile,
    exchange: OAuthExchangeResult,
) -> Result<AccountId, ApiError> {
    let identity_email = exchange.identity_email.trim().to_string();
    let email_patterns = vec![identity_email.clone()];
    let name = identity_email.clone();
    let seed = generate_account_id_seed(&name, &email_patterns);
    let account_id = allocate_unique_account_id(state.as_ref(), &seed)?;

    let secret_ref = account_secret_ref(&account_id);
    let timestamp = domain_now_iso8601().map_err(internal_error)?;
    let account = oauth_account_settings(
        account_id.clone(),
        profile.provider.clone(),
        name,
        identity_email,
        email_patterns,
        secret_ref.clone(),
        timestamp,
    )?;
    let encoded = exchange.token_set.encode().map_err(ServiceError::from)?;
    state
        .secret_store
        .save(&secret_ref, &encoded)
        .map_err(ServiceError::from)
        .map_err(ApiError::from)?;

    if let Err(error) = validate_account_settings(&account) {
        delete_managed_secret(state.as_ref(), Some(&secret_ref))?;
        return Err(error);
    }
    persist_new_account(state, &account, EVENT_TOPIC_ACCOUNT_CREATED).await?;
    Ok(account_id)
}

pub(crate) fn oauth_account_settings(
    account_id: AccountId,
    provider: ProviderHint,
    name: String,
    identity_email: String,
    email_patterns: Vec<String>,
    secret_ref: SecretRef,
    timestamp: String,
) -> Result<AccountSettings, ApiError> {
    let (imap, smtp) = oauth_provider_mail_transport(&provider)?;
    Ok(AccountSettings {
        id: account_id,
        name,
        full_name: None,
        email_patterns,
        driver: AccountDriver::ImapSmtp,
        enabled: true,
        appearance: None,
        transport: AccountTransportSettings {
            provider,
            auth: ProviderAuthKind::OAuth2,
            base_url: None,
            username: Some(identity_email),
            secret_ref: Some(secret_ref),
            imap: Some(imap),
            smtp: Some(smtp),
        },
        created_at: timestamp.clone(),
        updated_at: timestamp,
    })
}

pub(crate) fn oauth_provider_mail_transport(
    provider: &ProviderHint,
) -> Result<(ImapTransportSettings, SmtpTransportSettings), ApiError> {
    OAuthProviderProfile::for_provider(provider)
        .and_then(|profile| profile.default_mail_transport())
        .ok_or_else(|| {
            ApiError::new(
                StatusCode::BAD_REQUEST,
                ApiErrorCode::InvalidProvider,
                "provider does not support built-in OAuth account creation",
            )
        })
}

pub(crate) async fn persist_oauth_token_set(
    state: &Arc<AppState>,
    account_id: &AccountId,
    token_set: OAuthTokenSet,
) -> Result<(), ApiError> {
    let mut account = load_account(state.as_ref(), account_id)?;
    let previous_secret_ref = account.transport.secret_ref.clone();
    let secret_ref = previous_secret_ref
        .as_ref()
        .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
        .cloned()
        .unwrap_or_else(|| account_secret_ref(&account.id));
    let encoded = token_set.encode().map_err(ServiceError::from)?;

    account.transport.auth = ProviderAuthKind::OAuth2;
    account.transport.secret_ref = Some(secret_ref.clone());
    account.updated_at = domain_now_iso8601().map_err(internal_error)?;
    validate_account_settings(&account)?;

    match previous_secret_ref.as_ref() {
        Some(existing) if existing == &secret_ref => state
            .secret_store
            .update(&secret_ref, &encoded)
            .map_err(ServiceError::from)
            .map_err(ApiError::from)?,
        _ => {
            delete_managed_secret(state.as_ref(), previous_secret_ref.as_ref())?;
            state
                .secret_store
                .save(&secret_ref, &encoded)
                .map_err(ServiceError::from)
                .map_err(ApiError::from)?;
        }
    }

    state
        .service
        .save_source(&account)
        .map_err(ApiError::from_service_error)?;
    state.supervisor.start_account(&account).await;
    append_and_publish_account_event(state, account_id, EVENT_TOPIC_ACCOUNT_UPDATED)
        .map_err(store_error_to_api)?;

    Ok(())
}
