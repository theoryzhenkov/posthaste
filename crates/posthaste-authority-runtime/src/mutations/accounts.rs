use super::*;

impl AccountMutationService {
    pub async fn create_account(
        &self,
        request: CreateAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        let CreateAccountMutation {
            id,
            name,
            full_name,
            email_patterns,
            driver,
            enabled,
            appearance,
            transport,
            secret,
        } = request;
        let email_patterns = normalize_email_patterns(&email_patterns);
        let account_id = id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(AccountId::from)
            .unwrap_or_else(generate_account_id);

        let timestamp = domain_now_iso8601()
            .map_err(|error| RuntimeError::new(RuntimeErrorCode::Internal, error))?;
        let mut account = AccountSettings {
            id: account_id,
            name: name.trim().to_string(),
            full_name: normalize_optional(full_name.as_deref()),
            email_patterns,
            driver: driver.unwrap_or(AccountDriver::Jmap),
            enabled: enabled.unwrap_or(true),
            appearance: appearance.map(normalize_account_appearance),
            transport: account_transport_from_mutation(transport),
            created_at: timestamp.clone(),
            updated_at: timestamp,
        };
        account.transport.secret_ref =
            decide_secret_instruction(&account.id, None, &secret)?.resolved_secret_ref(None);
        validate_account_settings(&account)?;
        self.persist_new_account(&mut account, &secret).await?;
        self.read_account_overview(account.id.clone()).await
    }

    pub async fn patch_account(
        &self,
        account_id: AccountId,
        request: PatchAccountMutation,
    ) -> Result<AccountOverview, RuntimeError> {
        let mut account = self.load_account(&account_id)?;
        apply_account_patch(&mut account, &request);
        account.updated_at = domain_now_iso8601()
            .map_err(|error| RuntimeError::new(RuntimeErrorCode::Internal, error))?;
        let existing_secret_ref = account.transport.secret_ref.clone();
        let secret_request = request.secret.unwrap_or_default();
        account.transport.secret_ref =
            decide_secret_instruction(&account.id, existing_secret_ref.as_ref(), &secret_request)?
                .resolved_secret_ref(existing_secret_ref.as_ref());
        validate_account_settings(&account)?;
        let defer_secret_clear = secret_request.mode == SecretWriteMode::Clear;
        if !defer_secret_clear {
            self.apply_secret_instruction(
                &mut account,
                existing_secret_ref.as_ref(),
                &secret_request,
            )?;
        }

        self.service.save_source(&account)?;
        if defer_secret_clear {
            self.apply_secret_instruction(
                &mut account,
                existing_secret_ref.as_ref(),
                &secret_request,
            )?;
        }
        self.supervisor.start_account(&account).await;
        self.append_and_publish_event(
            &account_id,
            EVENT_TOPIC_ACCOUNT_UPDATED,
            account_event_payload(EVENT_TOPIC_ACCOUNT_UPDATED, &account_id),
        )?;
        self.read_account_overview(account_id).await
    }

    pub async fn create_oauth_account_from_exchange(
        &self,
        profile: &OAuthProviderProfile,
        exchange: OAuthExchangeResult,
    ) -> Result<AccountOverview, RuntimeError> {
        let identity_email = exchange.identity_email.trim().to_string();
        let encoded = exchange.token_set.encode().map_err(ServiceError::from)?;
        let (imap, smtp) = oauth_provider_mail_transport(&profile.provider)?;
        self.create_account(CreateAccountMutation {
            id: None,
            name: identity_email.clone(),
            full_name: None,
            email_patterns: vec![identity_email.clone()],
            driver: Some(AccountDriver::ImapSmtp),
            enabled: Some(true),
            appearance: None,
            transport: AccountTransportMutation {
                provider: Some(profile.provider.clone()),
                auth: Some(ProviderAuthKind::OAuth2),
                base_url: None,
                username: Some(identity_email),
                imap: Some(imap),
                smtp: Some(smtp),
            },
            secret: SecretWriteMutation {
                mode: SecretWriteMode::Replace,
                password: Some(encoded),
            },
        })
        .await
    }

    pub async fn persist_oauth_token_set(
        &self,
        account_id: AccountId,
        token_set: OAuthTokenSet,
    ) -> Result<AccountOverview, RuntimeError> {
        let encoded = token_set.encode().map_err(ServiceError::from)?;
        self.patch_account(
            account_id,
            PatchAccountMutation {
                name: None,
                full_name: None,
                email_patterns: None,
                driver: None,
                enabled: None,
                appearance: None,
                transport: Some(AccountTransportMutation {
                    provider: None,
                    auth: Some(ProviderAuthKind::OAuth2),
                    base_url: None,
                    username: None,
                    imap: None,
                    smtp: None,
                }),
                secret: Some(SecretWriteMutation {
                    mode: SecretWriteMode::Replace,
                    password: Some(encoded),
                }),
            },
        )
        .await
    }

    pub async fn delete_account(&self, account_id: AccountId) -> Result<(), RuntimeError> {
        let account = self.load_account(&account_id)?;
        self.delete_managed_secret(account.transport.secret_ref.as_ref())?;
        self.supervisor.remove_account(&account_id).await;
        self.service.delete_source(&account_id)?;
        self.append_and_publish_event(
            &account_id,
            EVENT_TOPIC_ACCOUNT_DELETED,
            account_event_payload(EVENT_TOPIC_ACCOUNT_DELETED, &account_id),
        )
    }

    pub async fn verify_account(
        &self,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        let account = self.load_account(&account_id)?;
        let result = self.supervisor.verify_account(&account).await?;
        Ok(AccountVerificationResult {
            ok: result.ok,
            identity_email: result.identity.map(|identity| identity.email),
            push_supported: result.push_supported,
        })
    }

    pub async fn set_account_enabled(
        &self,
        account_id: AccountId,
        enabled: bool,
    ) -> Result<(), RuntimeError> {
        let mut account = self.load_account(&account_id)?;
        account.enabled = enabled;
        account.updated_at = domain_now_iso8601()
            .map_err(|error| RuntimeError::new(RuntimeErrorCode::Internal, error))?;
        self.service.save_source(&account)?;
        self.supervisor.start_account(&account).await;
        self.append_and_publish_event(
            &account_id,
            EVENT_TOPIC_ACCOUNT_UPDATED,
            account_event_payload(EVENT_TOPIC_ACCOUNT_UPDATED, &account_id),
        )
    }

    pub async fn reload_config(&self) -> Result<(), RuntimeError> {
        let diff = self.service.reload_config()?;
        for id in &diff.removed_sources {
            self.supervisor.remove_account(id).await;
        }
        for id in diff.added_sources.iter().chain(diff.changed_sources.iter()) {
            if let Some(source) = self.service.get_source(id)? {
                self.supervisor.start_account(&source).await;
            }
        }

        let mut resources = vec![ResourceChange::config_reloaded()];
        resources.extend(
            diff.added_sources
                .iter()
                .map(|id| ResourceChange::account(ResourceOperation::Created, id)),
        );
        resources.extend(
            diff.changed_sources
                .iter()
                .map(|id| ResourceChange::account(ResourceOperation::Updated, id)),
        );
        resources.extend(
            diff.removed_sources
                .iter()
                .map(|id| ResourceChange::account(ResourceOperation::Deleted, id)),
        );
        self.append_and_publish_event(
            &AccountId::from(GLOBAL_EVENT_ACCOUNT_ID),
            EVENT_TOPIC_CONFIG_RELOADED,
            config_event_payload(
                resources,
                json!({
                    "addedSourceCount": diff.added_sources.len(),
                    "changedSourceCount": diff.changed_sources.len(),
                    "removedSourceCount": diff.removed_sources.len(),
                }),
            ),
        )?;
        Ok(())
    }

    async fn persist_new_account(
        &self,
        account: &mut AccountSettings,
        secret: &SecretWriteMutation,
    ) -> Result<(), RuntimeError> {
        self.service.insert_source(account)?;
        if let Err(error) = self.apply_secret_instruction(account, None, secret) {
            self.service.delete_source(&account.id).map_err(|rollback| {
                RuntimeError::new(
                    RuntimeErrorCode::Internal,
                    format!(
                        "failed to roll back account after secret write error: {rollback}; original error: {}",
                        error.envelope().message
                    ),
                )
            })?;
            return Err(error);
        }
        self.supervisor.start_account(account).await;
        self.append_and_publish_event(
            &account.id,
            EVENT_TOPIC_ACCOUNT_CREATED,
            account_event_payload(EVENT_TOPIC_ACCOUNT_CREATED, &account.id),
        )
    }

    fn load_account(&self, account_id: &AccountId) -> Result<AccountSettings, RuntimeError> {
        self.service
            .get_source(account_id)?
            .ok_or_else(|| RuntimeError::not_found("account not found"))
    }

    async fn read_account_overview(
        &self,
        account_id: AccountId,
    ) -> Result<AccountOverview, RuntimeError> {
        self.reads
            .get_account(account_id)
            .await?
            .ok_or_else(|| RuntimeError::not_found("account not found"))
    }
}

fn account_transport_from_mutation(mutation: AccountTransportMutation) -> AccountTransportSettings {
    AccountTransportSettings {
        provider: mutation.provider.unwrap_or_default(),
        auth: mutation.auth.unwrap_or_default(),
        base_url: mutation.base_url,
        username: mutation.username,
        secret_ref: None,
        imap: mutation.imap,
        smtp: mutation.smtp,
    }
}

fn oauth_provider_mail_transport(
    provider: &ProviderHint,
) -> Result<(ImapTransportSettings, SmtpTransportSettings), RuntimeError> {
    OAuthProviderProfile::for_provider(provider)
        .and_then(|profile| profile.default_mail_transport())
        .ok_or_else(|| {
            RuntimeError::invalid_account(
                "provider does not support built-in OAuth account creation",
            )
        })
}

fn apply_account_patch(account: &mut AccountSettings, request: &PatchAccountMutation) {
    let PatchAccountMutation {
        name,
        full_name,
        email_patterns,
        driver,
        enabled,
        appearance,
        transport,
        secret: _,
    } = request;

    if let Some(name) = name.as_deref() {
        account.name = name.trim().to_string();
    }
    if full_name.is_some() {
        account.full_name = normalize_optional(full_name.as_deref());
    }
    if let Some(email_patterns) = email_patterns {
        account.email_patterns = normalize_email_patterns(email_patterns);
    }
    replace_if_some(&mut account.driver, driver);
    replace_if_some(&mut account.enabled, enabled);
    if let Some(appearance) = appearance {
        account.appearance = Some(normalize_account_appearance(appearance.clone()));
    }
    if let Some(transport) = transport {
        apply_transport_patch(&mut account.transport, transport);
    }
}

fn apply_transport_patch(
    settings: &mut AccountTransportSettings,
    patch: &AccountTransportMutation,
) {
    let AccountTransportMutation {
        provider,
        auth,
        base_url,
        username,
        imap,
        smtp,
    } = patch;

    replace_if_some(&mut settings.provider, provider);
    replace_if_some(&mut settings.auth, auth);
    if base_url.is_some() {
        settings.base_url = normalize_optional(base_url.as_deref());
    }
    if username.is_some() {
        settings.username = normalize_optional(username.as_deref());
    }
    replace_optional_if_some(&mut settings.imap, imap);
    replace_optional_if_some(&mut settings.smtp, smtp);
}

fn replace_if_some<T: Clone>(target: &mut T, patch: &Option<T>) {
    if let Some(value) = patch {
        target.clone_from(value);
    }
}

fn replace_optional_if_some<T: Clone>(target: &mut Option<T>, patch: &Option<T>) {
    if patch.is_some() {
        target.clone_from(patch);
    }
}

fn normalize_optional(value: Option<&str>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn normalize_email_patterns(patterns: &[String]) -> Vec<String> {
    patterns
        .iter()
        .filter_map(|pattern| {
            let trimmed = pattern.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect()
}

fn normalize_account_appearance(appearance: AccountAppearance) -> AccountAppearance {
    match appearance {
        AccountAppearance::Initials {
            initials,
            color_hue,
        } => AccountAppearance::Initials {
            initials: normalize_initials(&initials),
            color_hue: color_hue.min(360),
        },
        AccountAppearance::Image {
            image_id,
            initials,
            color_hue,
        } => AccountAppearance::Image {
            image_id: image_id.trim().to_string(),
            initials: normalize_initials(&initials),
            color_hue: color_hue.min(360),
        },
    }
}

fn normalize_initials(value: &str) -> String {
    let words: Vec<&str> = value
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .collect();
    let raw = if words.len() >= 2 {
        words
            .iter()
            .take(2)
            .filter_map(|word| word.chars().next())
            .collect::<String>()
    } else {
        value
            .chars()
            .filter(|ch| !ch.is_whitespace())
            .take(2)
            .collect()
    };
    let normalized = raw.trim().to_uppercase();
    if normalized.is_empty() {
        "A".to_string()
    } else {
        normalized.chars().take(4).collect()
    }
}

fn validate_account_settings(account: &AccountSettings) -> Result<(), RuntimeError> {
    if account.id.as_str().trim().is_empty() {
        return Err(RuntimeError::invalid_account("account id is required"));
    }
    if account.name.trim().is_empty() {
        return Err(RuntimeError::invalid_account("account name is required"));
    }
    if account
        .email_patterns
        .iter()
        .any(|pattern| pattern.trim().is_empty())
    {
        return Err(RuntimeError::invalid_account(
            "email patterns must not be blank",
        ));
    }
    if matches!(account.driver, AccountDriver::Jmap) {
        if account
            .transport
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err(RuntimeError::account_base_url_required(
                "JMAP base URL is required",
            ));
        }
        if account.transport.secret_ref.is_none() {
            return Err(RuntimeError::account_secret_required(
                "JMAP secret must be configured before saving the account",
            ));
        }
    }
    if matches!(account.driver, AccountDriver::ImapSmtp) {
        if account
            .transport
            .username
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return Err(RuntimeError::account_username_required(
                "IMAP/SMTP username is required",
            ));
        }
        if account.transport.secret_ref.is_none() {
            return Err(RuntimeError::account_secret_required(
                "IMAP/SMTP secret must be configured before saving the account",
            ));
        }
        validate_endpoint("IMAP", account.transport.imap.as_ref())?;
        validate_endpoint("SMTP", account.transport.smtp.as_ref())?;
        if !account
            .email_patterns
            .iter()
            .any(|pattern| is_concrete_email_pattern(pattern))
        {
            return Err(RuntimeError::account_sender_required(
                "IMAP/SMTP accounts require a concrete sender email pattern",
            ));
        }
    }
    if let Some(
        AccountAppearance::Initials { initials, .. } | AccountAppearance::Image { initials, .. },
    ) = &account.appearance
    {
        if initials.trim().is_empty() {
            return Err(RuntimeError::invalid_account(
                "account appearance initials are required",
            ));
        }
    }
    Ok(())
}

trait EndpointLike {
    fn host(&self) -> &str;
    fn port(&self) -> u16;
}
impl EndpointLike for ImapTransportSettings {
    fn host(&self) -> &str {
        &self.host
    }
    fn port(&self) -> u16 {
        self.port
    }
}
impl EndpointLike for SmtpTransportSettings {
    fn host(&self) -> &str {
        &self.host
    }
    fn port(&self) -> u16 {
        self.port
    }
}

fn validate_endpoint<T: EndpointLike>(
    label: &str,
    endpoint: Option<&T>,
) -> Result<(), RuntimeError> {
    let endpoint = endpoint
        .ok_or_else(|| RuntimeError::invalid_account(format!("{label} endpoint is required")))?;
    if endpoint.host().trim().is_empty() {
        return Err(RuntimeError::invalid_account(format!(
            "{label} host is required"
        )));
    }
    if endpoint.port() == 0 {
        return Err(RuntimeError::invalid_account(format!(
            "{label} port must be greater than zero"
        )));
    }
    Ok(())
}

fn is_concrete_email_pattern(pattern: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern.contains('*') {
        return false;
    }
    pattern
        .split_once('@')
        .is_some_and(|(local, domain)| !local.is_empty() && !domain.is_empty())
}
