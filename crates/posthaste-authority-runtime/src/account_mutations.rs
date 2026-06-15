use std::sync::Arc;

use posthaste_domain::{
    now_iso8601 as domain_now_iso8601, AccountAppearance, AccountDriver, AccountId,
    AccountOverview, AccountSettings, AccountTransportSettings, AutomationAction, AutomationRule,
    CachePolicy, DomainEvent, ImapTransportSettings, MailService, MailStore, MailboxId,
    MessageSortField, ProviderAuthKind, ProviderHint, SecretKind, SecretRef, SecretStore,
    ServiceError, SmartMailbox, SmartMailboxId, SmartMailboxKind, SmtpTransportSettings,
    SortDirection, StoreError, EVENT_TOPIC_ACCOUNT_CREATED, EVENT_TOPIC_ACCOUNT_UPDATED,
    EVENT_TOPIC_CONFIG_RELOADED, EVENT_TOPIC_SETTINGS_UPDATED, EVENT_TOPIC_SMART_MAILBOX_CREATED,
    EVENT_TOPIC_SMART_MAILBOX_DELETED, EVENT_TOPIC_SMART_MAILBOX_RESET,
    EVENT_TOPIC_SMART_MAILBOX_UPDATED,
};
use posthaste_runtime_contract::{
    AccountTransportMutation, AccountVerificationResult, AutomationRulePreviewMutation,
    AutomationRulePreviewResult, CreateAccountMutation, CreateSmartMailboxMutation,
    PatchAccountMutation, PatchAppSettingsMutation, PatchSmartMailboxMutation, RuntimeAdapterError,
    RuntimeError, RuntimeErrorCode, SecretWriteMode, SecretWriteMutation,
};
use serde::Serialize;
use serde_json::json;
use tokio::sync::broadcast;

use crate::account_reads::AccountReadService;
use crate::oauth::{OAuthExchangeResult, OAuthProviderProfile, OAuthTokenSet};
use crate::supervisor::AccountSupervisor;

const GLOBAL_EVENT_ACCOUNT_ID: &str = "app";

pub struct AccountMutationService {
    service: Arc<MailService>,
    store: Arc<dyn MailStore>,
    secret_store: Arc<dyn SecretStore>,
    event_sender: broadcast::Sender<DomainEvent>,
    supervisor: Arc<AccountSupervisor>,
    reads: Arc<AccountReadService>,
}

impl AccountMutationService {
    pub fn new(
        service: Arc<MailService>,
        store: Arc<dyn MailStore>,
        secret_store: Arc<dyn SecretStore>,
        event_sender: broadcast::Sender<DomainEvent>,
        supervisor: Arc<AccountSupervisor>,
        reads: Arc<AccountReadService>,
    ) -> Self {
        Self {
            service,
            store,
            secret_store,
            event_sender,
            supervisor,
            reads,
        }
    }

    pub fn patch_app_settings(
        &self,
        request: PatchAppSettingsMutation,
    ) -> Result<posthaste_domain::AppSettings, RuntimeError> {
        let mut settings = self
            .service
            .get_app_settings()
            .map_err(service_error_to_runtime_error)?;
        if let Some(default_account_id) = &request.default_account_id {
            if let Some(default_account_id) = default_account_id {
                let account_id = AccountId::from(default_account_id.as_str());
                if self
                    .service
                    .get_source(&account_id)
                    .map_err(service_error_to_runtime_error)?
                    .is_none()
                {
                    return Err(runtime_error(
                        RuntimeErrorCode::InvalidAccount,
                        "default account must reference an existing account",
                    ));
                }
                settings.default_account_id = Some(account_id);
            } else {
                settings.default_account_id = None;
            }
        }
        if let Some(automation_rules) = &request.automation_rules {
            settings.automation_rules = normalize_automation_rules(automation_rules);
        }
        if let Some(automation_drafts) = &request.automation_drafts {
            settings.automation_drafts = normalize_automation_rules(automation_drafts);
        }
        if let Some(cache_policy) = &request.cache_policy {
            settings.cache_policy = normalize_cache_policy(cache_policy.clone());
        }
        validate_automation_rules(&settings.automation_rules)?;
        validate_automation_drafts(&settings.automation_rules, &settings.automation_drafts)?;

        let mut changed = Vec::new();
        if request.default_account_id.is_some() {
            changed.push("defaultAccount");
        }
        if request.automation_rules.is_some() {
            changed.push("automationRules");
        }
        if request.automation_drafts.is_some() {
            changed.push("automationDrafts");
        }
        if request.cache_policy.is_some() {
            changed.push("cachePolicy");
        }

        self.service
            .put_app_settings(&settings)
            .map_err(service_error_to_runtime_error)?;
        self.append_and_publish_config_event(
            EVENT_TOPIC_SETTINGS_UPDATED,
            vec![ResourceChange::app_settings_updated()],
            json!({
                "scope": "app",
                "changed": changed,
            }),
        )?;
        if request.automation_rules.is_some() {
            self.service
                .ensure_automation_backfills_for_current_rules()
                .map_err(service_error_to_runtime_error)?;
        }
        Ok(settings)
    }

    pub fn preview_automation_rule(
        &self,
        request: AutomationRulePreviewMutation,
    ) -> Result<AutomationRulePreviewResult, RuntimeError> {
        let (_, total) = self
            .service
            .count_messages_by_rule(&request.condition)
            .map_err(service_error_to_runtime_error)?;
        let page = self
            .service
            .query_message_page_by_rule(
                &request.condition,
                request.limit,
                None,
                MessageSortField::Date,
                SortDirection::Desc,
            )
            .map_err(service_error_to_runtime_error)?;
        Ok(AutomationRulePreviewResult {
            total,
            items: page.items,
        })
    }

    pub fn create_smart_mailbox(
        &self,
        request: CreateSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        let timestamp = domain_now_iso8601()
            .map_err(|error| runtime_error(RuntimeErrorCode::Internal, error))?;
        let smart_mailbox = SmartMailbox {
            id: SmartMailboxId::from(generate_smart_mailbox_id(&request.name)),
            name: request.name,
            position: request.position.unwrap_or(0),
            kind: SmartMailboxKind::User,
            default_key: None,
            parent_id: None,
            rule: request.rule,
            created_at: timestamp.clone(),
            updated_at: timestamp,
        };
        self.service
            .save_smart_mailbox(&smart_mailbox)
            .map_err(service_error_to_runtime_error)?;
        self.append_and_publish_config_event(
            EVENT_TOPIC_SMART_MAILBOX_CREATED,
            vec![ResourceChange::smart_mailbox(
                ResourceOperation::Created,
                &smart_mailbox.id,
            )],
            json!({ "smartMailboxId": smart_mailbox.id.as_str() }),
        )?;
        Ok(smart_mailbox)
    }

    pub fn patch_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
        request: PatchSmartMailboxMutation,
    ) -> Result<SmartMailbox, RuntimeError> {
        let mut smart_mailbox = self
            .service
            .get_smart_mailbox(&smart_mailbox_id)
            .map_err(service_error_to_runtime_error)?;
        if let Some(name) = request.name {
            smart_mailbox.name = name;
        }
        if let Some(position) = request.position {
            smart_mailbox.position = position;
        }
        if let Some(rule) = request.rule {
            smart_mailbox.rule = rule;
        }
        smart_mailbox.updated_at = domain_now_iso8601()
            .map_err(|error| runtime_error(RuntimeErrorCode::Internal, error))?;
        self.service
            .save_smart_mailbox(&smart_mailbox)
            .map_err(service_error_to_runtime_error)?;
        self.append_and_publish_config_event(
            EVENT_TOPIC_SMART_MAILBOX_UPDATED,
            vec![ResourceChange::smart_mailbox(
                ResourceOperation::Updated,
                &smart_mailbox.id,
            )],
            json!({ "smartMailboxId": smart_mailbox.id.as_str() }),
        )?;
        Ok(smart_mailbox)
    }

    pub fn delete_smart_mailbox(
        &self,
        smart_mailbox_id: SmartMailboxId,
    ) -> Result<(), RuntimeError> {
        self.service
            .delete_smart_mailbox(&smart_mailbox_id)
            .map_err(service_error_to_runtime_error)?;
        self.append_and_publish_config_event(
            EVENT_TOPIC_SMART_MAILBOX_DELETED,
            vec![ResourceChange::smart_mailbox(
                ResourceOperation::Deleted,
                &smart_mailbox_id,
            )],
            json!({ "smartMailboxId": smart_mailbox_id.as_str() }),
        )
    }

    pub fn reset_default_smart_mailboxes(
        &self,
    ) -> Result<Vec<posthaste_domain::SmartMailboxSummary>, RuntimeError> {
        self.service
            .reset_default_smart_mailboxes()
            .map_err(service_error_to_runtime_error)?;
        self.append_and_publish_config_event(
            EVENT_TOPIC_SMART_MAILBOX_RESET,
            vec![ResourceChange::smart_mailbox_reset()],
            json!({ "scope": "smartMailboxes" }),
        )?;
        self.reads
            .list_smart_mailboxes()
            .map_err(service_error_to_runtime_error)
    }

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
        let account_id = match id {
            Some(id) if !id.trim().is_empty() => AccountId::from(id.trim()),
            _ => {
                let seed = generate_account_id_seed(&name, &email_patterns);
                self.allocate_unique_account_id(&seed)?
            }
        };
        if self
            .service
            .get_source(&account_id)
            .map_err(service_error_to_runtime_error)?
            .is_some()
        {
            return Err(runtime_error(
                RuntimeErrorCode::Conflict,
                "account already exists",
            ));
        }

        let timestamp = domain_now_iso8601()
            .map_err(|error| runtime_error(RuntimeErrorCode::Internal, error))?;
        let mut account = AccountSettings {
            id: account_id,
            name: name.trim().to_string(),
            full_name: normalize_optional(full_name),
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
        self.apply_secret_instruction(&mut account, None, &secret)?;
        self.persist_new_account(&account).await?;
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
            .map_err(|error| runtime_error(RuntimeErrorCode::Internal, error))?;
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

        self.service
            .save_source(&account)
            .map_err(service_error_to_runtime_error)?;
        if defer_secret_clear {
            self.apply_secret_instruction(
                &mut account,
                existing_secret_ref.as_ref(),
                &secret_request,
            )?;
        }
        self.supervisor.start_account(&account).await;
        self.append_and_publish_account_event(&account_id, EVENT_TOPIC_ACCOUNT_UPDATED)?;
        self.read_account_overview(account_id).await
    }

    pub async fn create_oauth_account_from_exchange(
        &self,
        profile: &OAuthProviderProfile,
        exchange: OAuthExchangeResult,
    ) -> Result<AccountOverview, RuntimeError> {
        let identity_email = exchange.identity_email.trim().to_string();
        let encoded = exchange
            .token_set
            .encode()
            .map_err(ServiceError::from)
            .map_err(service_error_to_runtime_error)?;
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
        let encoded = token_set
            .encode()
            .map_err(ServiceError::from)
            .map_err(service_error_to_runtime_error)?;
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

    pub async fn verify_account(
        &self,
        account_id: AccountId,
    ) -> Result<AccountVerificationResult, RuntimeError> {
        let account = self.load_account(&account_id)?;
        let result = self
            .supervisor
            .verify_account(&account)
            .await
            .map_err(service_error_to_runtime_error)?;
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
            .map_err(|error| runtime_error(RuntimeErrorCode::Internal, error))?;
        self.service
            .save_source(&account)
            .map_err(service_error_to_runtime_error)?;
        self.supervisor.start_account(&account).await;
        self.append_and_publish_account_event(&account_id, EVENT_TOPIC_ACCOUNT_UPDATED)
    }

    pub async fn reload_config(&self) -> Result<(), RuntimeError> {
        let diff = self
            .service
            .reload_config()
            .map_err(service_error_to_runtime_error)?;
        for id in &diff.removed_sources {
            self.supervisor.remove_account(id).await;
        }
        for id in diff.added_sources.iter().chain(diff.changed_sources.iter()) {
            if let Some(source) = self
                .service
                .get_source(id)
                .map_err(service_error_to_runtime_error)?
            {
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
        self.append_and_publish_config_event(
            EVENT_TOPIC_CONFIG_RELOADED,
            resources,
            json!({
                "addedSourceCount": diff.added_sources.len(),
                "changedSourceCount": diff.changed_sources.len(),
                "removedSourceCount": diff.removed_sources.len(),
            }),
        )?;
        Ok(())
    }

    fn allocate_unique_account_id(&self, seed: &str) -> Result<AccountId, RuntimeError> {
        let mut candidate = AccountId::from(seed);
        let mut suffix = 2;
        while self
            .service
            .get_source(&candidate)
            .map_err(service_error_to_runtime_error)?
            .is_some()
        {
            candidate = AccountId::from(format!("{seed}-{suffix}"));
            suffix += 1;
        }
        Ok(candidate)
    }

    async fn persist_new_account(&self, account: &AccountSettings) -> Result<(), RuntimeError> {
        if let Err(error) = self.service.save_source(account) {
            self.delete_managed_secret(account.transport.secret_ref.as_ref())?;
            return Err(service_error_to_runtime_error(error));
        }
        self.supervisor.start_account(account).await;
        self.append_and_publish_account_event(&account.id, EVENT_TOPIC_ACCOUNT_CREATED)
    }

    fn load_account(&self, account_id: &AccountId) -> Result<AccountSettings, RuntimeError> {
        self.service
            .get_source(account_id)
            .map_err(service_error_to_runtime_error)?
            .ok_or_else(|| runtime_error(RuntimeErrorCode::NotFound, "account not found"))
    }

    async fn read_account_overview(
        &self,
        account_id: AccountId,
    ) -> Result<AccountOverview, RuntimeError> {
        self.reads
            .get_account(account_id)
            .await
            .map_err(service_error_to_runtime_error)?
            .ok_or_else(|| runtime_error(RuntimeErrorCode::NotFound, "account not found"))
    }

    fn apply_secret_instruction(
        &self,
        account: &mut AccountSettings,
        previous_secret_ref: Option<&SecretRef>,
        secret: &SecretWriteMutation,
    ) -> Result<(), RuntimeError> {
        let decision = decide_secret_instruction(&account.id, previous_secret_ref, secret)?;
        match &decision.store_instruction {
            SecretStoreInstruction::None => {}
            SecretStoreInstruction::Save {
                secret_ref,
                password,
            } => self
                .secret_store
                .save(secret_ref, password)
                .map_err(ServiceError::from)
                .map_err(service_error_to_runtime_error)?,
            SecretStoreInstruction::Update {
                secret_ref,
                password,
            } => self
                .secret_store
                .update(secret_ref, password)
                .map_err(ServiceError::from)
                .map_err(service_error_to_runtime_error)?,
            SecretStoreInstruction::Delete { secret_ref } => self
                .secret_store
                .delete(secret_ref)
                .map_err(ServiceError::from)
                .map_err(service_error_to_runtime_error)?,
        }
        match decision.account_secret_ref {
            AccountSecretRefUpdate::Preserve => {}
            AccountSecretRefUpdate::Set(secret_ref) => account.transport.secret_ref = secret_ref,
        }
        Ok(())
    }

    fn delete_managed_secret(&self, secret_ref: Option<&SecretRef>) -> Result<(), RuntimeError> {
        if let Some(secret_ref) = secret_ref {
            if matches!(secret_ref.kind, SecretKind::Os) {
                self.secret_store
                    .delete(secret_ref)
                    .map_err(ServiceError::from)
                    .map_err(service_error_to_runtime_error)?;
            }
        }
        Ok(())
    }

    fn publish_events(&self, events: &[DomainEvent]) {
        for event in events {
            let _ = self.event_sender.send(event.clone());
        }
    }

    fn append_and_publish_account_event(
        &self,
        account_id: &AccountId,
        topic: &str,
    ) -> Result<(), RuntimeError> {
        let operation = account_operation_from_topic(topic);
        let event = self
            .store
            .append_event(
                account_id,
                topic,
                None,
                None,
                json!({
                    "accountId": account_id.as_str(),
                    "resources": [ResourceChange::account(operation, account_id)],
                }),
            )
            .map_err(store_error_to_runtime_error)?;
        self.publish_events(&[event]);
        Ok(())
    }

    fn append_and_publish_config_event(
        &self,
        topic: &str,
        resources: Vec<ResourceChange>,
        extra: serde_json::Value,
    ) -> Result<(), RuntimeError> {
        let mut payload = match extra {
            serde_json::Value::Object(map) => map,
            _ => serde_json::Map::new(),
        };
        payload.insert("resources".to_string(), json!(resources));
        let event = self
            .store
            .append_event(
                &AccountId::from(GLOBAL_EVENT_ACCOUNT_ID),
                topic,
                None,
                None,
                serde_json::Value::Object(payload),
            )
            .map_err(store_error_to_runtime_error)?;
        self.publish_events(&[event]);
        Ok(())
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
            runtime_error(
                RuntimeErrorCode::InvalidAccount,
                "provider does not support built-in OAuth account creation",
            )
        })
}

fn apply_account_patch(account: &mut AccountSettings, request: &PatchAccountMutation) {
    if let Some(name) = &request.name {
        account.name = name.trim().to_string();
    }
    if let Some(full_name) = &request.full_name {
        account.full_name = normalize_optional(Some(full_name.clone()));
    }
    if let Some(email_patterns) = &request.email_patterns {
        account.email_patterns = normalize_email_patterns(email_patterns);
    }
    if let Some(driver) = &request.driver {
        account.driver = driver.clone();
    }
    if let Some(enabled) = request.enabled {
        account.enabled = enabled;
    }
    if let Some(appearance) = &request.appearance {
        account.appearance = Some(normalize_account_appearance(appearance.clone()));
    }
    if let Some(transport) = &request.transport {
        if let Some(provider) = &transport.provider {
            account.transport.provider = provider.clone();
        }
        if let Some(auth) = &transport.auth {
            account.transport.auth = auth.clone();
        }
        if transport.base_url.is_some() {
            account.transport.base_url = normalize_optional(transport.base_url.clone());
        }
        if transport.username.is_some() {
            account.transport.username = normalize_optional(transport.username.clone());
        }
        if transport.imap.is_some() {
            account.transport.imap = transport.imap.clone();
        }
        if transport.smtp.is_some() {
            account.transport.smtp = transport.smtp.clone();
        }
    }
}

fn normalize_optional(value: Option<String>) -> Option<String> {
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

fn normalize_cache_policy(mut policy: CachePolicy) -> CachePolicy {
    policy.hard_cap_bytes = policy.hard_cap_bytes.max(policy.soft_cap_bytes);
    policy
}

fn normalize_automation_rules(rules: &[AutomationRule]) -> Vec<AutomationRule> {
    rules
        .iter()
        .map(|rule| AutomationRule {
            id: rule.id.trim().to_string(),
            name: rule.name.trim().to_string(),
            enabled: rule.enabled,
            triggers: rule.triggers.clone(),
            condition: rule.condition.clone(),
            actions: rule
                .actions
                .iter()
                .map(normalize_automation_action)
                .collect(),
            backfill: rule.backfill,
        })
        .collect()
}

fn normalize_automation_action(action: &AutomationAction) -> AutomationAction {
    match action {
        AutomationAction::ApplyTag { tag } => AutomationAction::ApplyTag {
            tag: tag.trim().to_string(),
        },
        AutomationAction::RemoveTag { tag } => AutomationAction::RemoveTag {
            tag: tag.trim().to_string(),
        },
        AutomationAction::MarkRead => AutomationAction::MarkRead,
        AutomationAction::MarkUnread => AutomationAction::MarkUnread,
        AutomationAction::Flag => AutomationAction::Flag,
        AutomationAction::Unflag => AutomationAction::Unflag,
        AutomationAction::MoveToMailbox { mailbox_id } => AutomationAction::MoveToMailbox {
            mailbox_id: MailboxId::from(mailbox_id.as_str().trim()),
        },
    }
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

fn validate_automation_rules(rules: &[AutomationRule]) -> Result<(), RuntimeError> {
    let mut ids = std::collections::BTreeSet::new();
    for rule in rules {
        if rule.id.trim().is_empty() {
            return Err(runtime_error(
                RuntimeErrorCode::InvalidAccount,
                "automation rule id is required",
            ));
        }
        if !ids.insert(rule.id.trim().to_string()) {
            return Err(runtime_error(
                RuntimeErrorCode::InvalidAccount,
                "automation rule ids must be unique",
            ));
        }
        if rule.name.trim().is_empty() {
            return Err(runtime_error(
                RuntimeErrorCode::InvalidAccount,
                "automation rule name is required",
            ));
        }
        if rule.triggers.is_empty() {
            return Err(runtime_error(
                RuntimeErrorCode::InvalidAccount,
                "automation rule must include at least one trigger",
            ));
        }
        if rule.actions.is_empty() {
            return Err(runtime_error(
                RuntimeErrorCode::InvalidAccount,
                "automation rule must include at least one action",
            ));
        }
        for action in &rule.actions {
            match action {
                AutomationAction::ApplyTag { tag } | AutomationAction::RemoveTag { tag }
                    if tag.trim().is_empty() || tag.starts_with('$') =>
                {
                    return Err(runtime_error(
                        RuntimeErrorCode::InvalidAccount,
                        "automation tag must be a non-system keyword",
                    ));
                }
                AutomationAction::MoveToMailbox { mailbox_id }
                    if mailbox_id.as_str().trim().is_empty() =>
                {
                    return Err(runtime_error(
                        RuntimeErrorCode::InvalidAccount,
                        "automation target mailbox id is required",
                    ));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn validate_automation_drafts(
    active_rules: &[AutomationRule],
    draft_rules: &[AutomationRule],
) -> Result<(), RuntimeError> {
    let mut ids = std::collections::BTreeSet::new();
    for rule in active_rules {
        ids.insert(rule.id.trim().to_string());
    }
    for rule in draft_rules {
        if rule.id.trim().is_empty() {
            return Err(runtime_error(
                RuntimeErrorCode::InvalidAccount,
                "automation draft id is required",
            ));
        }
        if !ids.insert(rule.id.trim().to_string()) {
            return Err(runtime_error(
                RuntimeErrorCode::InvalidAccount,
                "automation rule and draft ids must be unique",
            ));
        }
    }
    Ok(())
}

fn validate_account_settings(account: &AccountSettings) -> Result<(), RuntimeError> {
    if account.id.as_str().trim().is_empty() {
        return Err(runtime_error(
            RuntimeErrorCode::InvalidAccount,
            "account id is required",
        ));
    }
    if account.name.trim().is_empty() {
        return Err(runtime_error(
            RuntimeErrorCode::InvalidAccount,
            "account name is required",
        ));
    }
    if account
        .email_patterns
        .iter()
        .any(|pattern| pattern.trim().is_empty())
    {
        return Err(runtime_error(
            RuntimeErrorCode::InvalidAccount,
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
            return Err(runtime_error(
                RuntimeErrorCode::AccountBaseUrlRequired,
                "JMAP base URL is required",
            ));
        }
        if account.transport.secret_ref.is_none() {
            return Err(runtime_error(
                RuntimeErrorCode::AccountSecretRequired,
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
            return Err(runtime_error(
                RuntimeErrorCode::AccountUsernameRequired,
                "IMAP/SMTP username is required",
            ));
        }
        if account.transport.secret_ref.is_none() {
            return Err(runtime_error(
                RuntimeErrorCode::AccountSecretRequired,
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
            return Err(runtime_error(
                RuntimeErrorCode::AccountSenderRequired,
                "IMAP/SMTP accounts require a concrete sender email pattern",
            ));
        }
    }
    if let Some(
        AccountAppearance::Initials { initials, .. } | AccountAppearance::Image { initials, .. },
    ) = &account.appearance
    {
        if initials.trim().is_empty() {
            return Err(runtime_error(
                RuntimeErrorCode::InvalidAccount,
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
    let endpoint = endpoint.ok_or_else(|| {
        runtime_error(
            RuntimeErrorCode::InvalidAccount,
            format!("{label} endpoint is required"),
        )
    })?;
    if endpoint.host().trim().is_empty() {
        return Err(runtime_error(
            RuntimeErrorCode::InvalidAccount,
            format!("{label} host is required"),
        ));
    }
    if endpoint.port() == 0 {
        return Err(runtime_error(
            RuntimeErrorCode::InvalidAccount,
            format!("{label} port must be greater than zero"),
        ));
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

struct SecretInstructionDecision<'a> {
    account_secret_ref: AccountSecretRefUpdate,
    store_instruction: SecretStoreInstruction<'a>,
}
impl SecretInstructionDecision<'_> {
    fn resolved_secret_ref(&self, previous_secret_ref: Option<&SecretRef>) -> Option<SecretRef> {
        match &self.account_secret_ref {
            AccountSecretRefUpdate::Preserve => previous_secret_ref.cloned(),
            AccountSecretRefUpdate::Set(secret_ref) => secret_ref.clone(),
        }
    }
}

enum AccountSecretRefUpdate {
    Preserve,
    Set(Option<SecretRef>),
}
enum SecretStoreInstruction<'a> {
    None,
    Save {
        secret_ref: SecretRef,
        password: &'a str,
    },
    Update {
        secret_ref: SecretRef,
        password: &'a str,
    },
    Delete {
        secret_ref: SecretRef,
    },
}

fn decide_secret_instruction<'a>(
    account_id: &AccountId,
    previous_secret_ref: Option<&SecretRef>,
    secret: &'a SecretWriteMutation,
) -> Result<SecretInstructionDecision<'a>, RuntimeError> {
    validate_secret_request(secret)?;
    let decision = match secret.mode {
        SecretWriteMode::Keep => SecretInstructionDecision {
            account_secret_ref: previous_secret_ref
                .cloned()
                .map(|secret_ref| AccountSecretRefUpdate::Set(Some(secret_ref)))
                .unwrap_or(AccountSecretRefUpdate::Preserve),
            store_instruction: SecretStoreInstruction::None,
        },
        SecretWriteMode::Replace => {
            let password = required_secret_password(secret)?;
            let secret_ref = previous_secret_ref
                .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
                .cloned()
                .unwrap_or_else(|| account_secret_ref(account_id));
            let store_instruction = match previous_secret_ref {
                Some(existing) if existing == &secret_ref => SecretStoreInstruction::Update {
                    secret_ref: secret_ref.clone(),
                    password,
                },
                _ => SecretStoreInstruction::Save {
                    secret_ref: secret_ref.clone(),
                    password,
                },
            };
            SecretInstructionDecision {
                account_secret_ref: AccountSecretRefUpdate::Set(Some(secret_ref)),
                store_instruction,
            }
        }
        SecretWriteMode::Clear => SecretInstructionDecision {
            account_secret_ref: AccountSecretRefUpdate::Set(None),
            store_instruction: previous_secret_ref
                .filter(|secret_ref| matches!(secret_ref.kind, SecretKind::Os))
                .cloned()
                .map(|secret_ref| SecretStoreInstruction::Delete { secret_ref })
                .unwrap_or(SecretStoreInstruction::None),
        },
    };
    Ok(decision)
}

fn validate_secret_request(secret: &SecretWriteMutation) -> Result<(), RuntimeError> {
    match secret.mode {
        SecretWriteMode::Keep => {
            if secret.password.is_some() {
                return Err(runtime_error(
                    RuntimeErrorCode::InvalidSecret,
                    "secret.password is only allowed when secret.mode is replace",
                ));
            }
        }
        SecretWriteMode::Replace => {
            required_secret_password(secret)?;
        }
        SecretWriteMode::Clear => {
            if secret.password.is_some() {
                return Err(runtime_error(
                    RuntimeErrorCode::InvalidSecret,
                    "secret.password is not allowed when secret.mode is clear",
                ));
            }
        }
    }
    Ok(())
}

fn required_secret_password(secret: &SecretWriteMutation) -> Result<&str, RuntimeError> {
    secret
        .password
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            runtime_error(
                RuntimeErrorCode::InvalidSecret,
                "secret.password is required when secret.mode is replace",
            )
        })
}

fn account_secret_ref(account_id: &AccountId) -> SecretRef {
    SecretRef {
        kind: SecretKind::Os,
        key: format!("account:{}", account_id.as_str()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ResourceKind {
    Account,
    AppSettings,
    Config,
    SmartMailbox,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ResourceOperation {
    Created,
    Updated,
    Deleted,
    Reloaded,
    Reset,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResourceChange {
    kind: ResourceKind,
    operation: ResourceOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    account_id: Option<String>,
}
impl ResourceChange {
    fn account(operation: ResourceOperation, account_id: &AccountId) -> Self {
        Self {
            kind: ResourceKind::Account,
            operation,
            id: Some(account_id.as_str().to_string()),
            account_id: Some(account_id.as_str().to_string()),
        }
    }
    fn config_reloaded() -> Self {
        Self {
            kind: ResourceKind::Config,
            operation: ResourceOperation::Reloaded,
            id: None,
            account_id: None,
        }
    }

    fn app_settings_updated() -> Self {
        Self {
            kind: ResourceKind::AppSettings,
            operation: ResourceOperation::Updated,
            id: None,
            account_id: None,
        }
    }

    fn smart_mailbox(operation: ResourceOperation, smart_mailbox_id: &SmartMailboxId) -> Self {
        Self {
            kind: ResourceKind::SmartMailbox,
            operation,
            id: Some(smart_mailbox_id.as_str().to_string()),
            account_id: None,
        }
    }

    fn smart_mailbox_reset() -> Self {
        Self {
            kind: ResourceKind::SmartMailbox,
            operation: ResourceOperation::Reset,
            id: None,
            account_id: None,
        }
    }
}

fn account_operation_from_topic(topic: &str) -> ResourceOperation {
    match topic {
        EVENT_TOPIC_ACCOUNT_CREATED => ResourceOperation::Created,
        _ => ResourceOperation::Updated,
    }
}

fn generate_smart_mailbox_id(name: &str) -> String {
    let slug = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|char| {
            if char.is_ascii_alphanumeric() {
                char
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    format!(
        "sm-{}-{}",
        if slug.is_empty() {
            "mailbox"
        } else {
            slug.as_str()
        },
        uuid::Uuid::new_v4()
    )
}

fn generate_account_id_seed(name: &str, email_patterns: &[String]) -> String {
    let seed = email_patterns
        .iter()
        .find_map(|pattern| pattern.split_once('@').map(|(local, _)| local.to_string()))
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| name.to_string());
    let slug = seed
        .trim()
        .to_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    let slug = slug
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        format!("account-{}", uuid::Uuid::new_v4())
    } else {
        slug
    }
}

pub(crate) fn service_error_to_runtime_error(error: ServiceError) -> RuntimeError {
    let code = match error.kind() {
        posthaste_domain::ServiceErrorKind::NotFound => RuntimeErrorCode::NotFound,
        posthaste_domain::ServiceErrorKind::Conflict => RuntimeErrorCode::Conflict,
        posthaste_domain::ServiceErrorKind::StateMismatch => RuntimeErrorCode::StateMismatch,
        posthaste_domain::ServiceErrorKind::AuthError => RuntimeErrorCode::Unauthorized,
        posthaste_domain::ServiceErrorKind::GatewayUnavailable => {
            RuntimeErrorCode::ProviderUnavailable
        }
        posthaste_domain::ServiceErrorKind::NetworkError => RuntimeErrorCode::NetworkError,
        posthaste_domain::ServiceErrorKind::CannotCalculateChanges => {
            RuntimeErrorCode::CannotCalculateChanges
        }
        posthaste_domain::ServiceErrorKind::GatewayRejected => RuntimeErrorCode::GatewayRejected,
        posthaste_domain::ServiceErrorKind::SecretUnavailable => {
            RuntimeErrorCode::SecretUnavailable
        }
        posthaste_domain::ServiceErrorKind::SecretUnsupported => {
            RuntimeErrorCode::SecretUnsupported
        }
        posthaste_domain::ServiceErrorKind::StorageFailure => RuntimeErrorCode::StorageFailure,
        posthaste_domain::ServiceErrorKind::ConfigValidation => RuntimeErrorCode::ConfigValidation,
        posthaste_domain::ServiceErrorKind::ConfigIo => RuntimeErrorCode::ConfigIo,
        posthaste_domain::ServiceErrorKind::ConfigParse => RuntimeErrorCode::ConfigParse,
    };
    runtime_error(code, error.to_string())
}

pub(crate) fn store_error_to_runtime_error(error: StoreError) -> RuntimeError {
    runtime_error(RuntimeErrorCode::Internal, error.to_string())
}

fn runtime_error(code: RuntimeErrorCode, message: impl Into<String>) -> RuntimeError {
    RuntimeError(RuntimeAdapterError {
        code,
        message: message.into(),
        retryable: false,
        correlation_id: None,
        details: serde_json::Value::Null,
    })
}
