use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use posthaste_contract_core::{AccountScopeRequest, RuntimeAccountList};
use posthaste_domain_model::{
    AccountAppearance, AccountConnectionOverview, AccountId, AccountOverview,
    AccountRuntimeOverview, AccountSettings, AppSettings, MailboxSummary, ProviderAuthKind,
    SecretKind, SecretRef, SecretStatus, SecretStorage, ServiceError, SmartMailbox, SmartMailboxId,
    SmartMailboxSummary, StoreError, TagSummary,
};
use posthaste_domain_service::MailService;

use crate::supervisor::AccountSupervisor;

#[async_trait]
pub trait AccountRuntimeOverviewProvider: Send + Sync {
    async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview;
}

pub struct DefaultAccountRuntimeOverviewProvider;

#[async_trait]
impl AccountRuntimeOverviewProvider for AccountSupervisor {
    async fn runtime_overview(&self, account_id: &AccountId) -> AccountRuntimeOverview {
        AccountSupervisor::runtime_overview(self, account_id).await
    }
}

#[async_trait]
impl AccountRuntimeOverviewProvider for DefaultAccountRuntimeOverviewProvider {
    async fn runtime_overview(&self, _account_id: &AccountId) -> AccountRuntimeOverview {
        AccountRuntimeOverview::default()
    }
}

pub struct AccountReadService {
    service: Arc<MailService>,
    runtime_status: Arc<dyn AccountRuntimeOverviewProvider>,
}

impl AccountReadService {
    pub fn new(
        service: Arc<MailService>,
        runtime_status: Arc<dyn AccountRuntimeOverviewProvider>,
    ) -> Self {
        Self {
            service,
            runtime_status,
        }
    }

    pub fn app_settings(&self) -> Result<AppSettings, ServiceError> {
        self.service.get_app_settings()
    }

    pub async fn list_accounts(&self) -> Result<RuntimeAccountList, ServiceError> {
        let settings = self.service.get_app_settings()?;
        let accounts = self.service.list_sources()?;
        let mut ids = Vec::with_capacity(accounts.len());
        let mut enabled_ids = Vec::new();
        let mut items = Vec::with_capacity(accounts.len());
        for account in accounts {
            ids.push(account.id.clone());
            if account.enabled {
                enabled_ids.push(account.id.clone());
            }
            items.push(account_overview(&settings, account, self.runtime_status.as_ref()).await);
        }
        Ok(RuntimeAccountList {
            ids,
            enabled_ids,
            items,
        })
    }

    pub async fn get_account(
        &self,
        account_id: AccountId,
    ) -> Result<Option<AccountOverview>, ServiceError> {
        let settings = self.service.get_app_settings()?;
        let Some(account) = self.service.get_source(&account_id)? else {
            return Ok(None);
        };
        Ok(Some(
            account_overview(&settings, account, self.runtime_status.as_ref()).await,
        ))
    }

    pub fn resolve_account_scope(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<Vec<AccountId>, ServiceError> {
        match scope {
            AccountScopeRequest::Explicit { account_ids } => Ok(account_ids),
            AccountScopeRequest::EnabledAccounts => Ok(self
                .service
                .list_sources()?
                .into_iter()
                .filter(|account| account.enabled)
                .map(|account| account.id)
                .collect()),
        }
    }

    pub fn list_mailboxes(
        &self,
        scope: AccountScopeRequest,
    ) -> Result<BTreeMap<AccountId, Vec<MailboxSummary>>, ServiceError> {
        let account_ids = self.resolve_account_scope(scope)?;
        let mut by_account_id = BTreeMap::new();
        for account_id in account_ids {
            if self.service.get_source(&account_id)?.is_none() {
                return Err(
                    StoreError::NotFound(format!("account:{}", account_id.as_str())).into(),
                );
            }
            let mailboxes = self.service.list_mailboxes(&account_id)?;
            by_account_id.insert(account_id, mailboxes);
        }
        Ok(by_account_id)
    }

    pub fn list_smart_mailboxes(&self) -> Result<Vec<SmartMailboxSummary>, ServiceError> {
        self.service.list_smart_mailboxes()
    }

    pub fn get_smart_mailbox(
        &self,
        smart_mailbox_id: &SmartMailboxId,
    ) -> Result<SmartMailbox, ServiceError> {
        self.service.get_smart_mailbox(smart_mailbox_id)
    }

    pub fn list_tags(&self, scope: AccountScopeRequest) -> Result<Vec<TagSummary>, ServiceError> {
        let account_ids = self.resolve_account_scope(scope)?;
        self.service.list_merged_tags(&account_ids)
    }
}

async fn account_overview(
    settings: &AppSettings,
    account: AccountSettings,
    runtime_provider: &dyn AccountRuntimeOverviewProvider,
) -> AccountOverview {
    let runtime = runtime_provider.runtime_overview(&account.id).await;
    AccountOverview {
        id: account.id.clone(),
        name: account.name.clone(),
        full_name: account.full_name.clone(),
        signature: account.signature.clone(),
        email_patterns: account.email_patterns.clone(),
        driver: account.driver.clone(),
        enabled: account.enabled,
        appearance: account
            .appearance
            .clone()
            .map(normalize_account_appearance)
            .unwrap_or_else(|| default_account_appearance(&account)),
        connection: account_connection_overview(&account),
        created_at: account.created_at.clone(),
        updated_at: account.updated_at.clone(),
        is_default: settings.default_account_id.as_ref() == Some(&account.id),
        runtime,
    }
}

fn account_connection_overview(account: &AccountSettings) -> AccountConnectionOverview {
    let secret = secret_status(account.transport.secret_ref.as_ref());
    match account.transport.auth {
        ProviderAuthKind::OAuth2 => AccountConnectionOverview::ManagedOAuth {
            provider: account.transport.provider.clone(),
            provider_kind: account.transport.provider_kind(),
            auth: account.transport.auth.clone(),
            username: account.transport.username.clone(),
            imap: account.transport.imap.clone(),
            smtp: account.transport.smtp.clone(),
            secret,
        },
        ProviderAuthKind::Password | ProviderAuthKind::AppPassword => {
            AccountConnectionOverview::ManualCredentials {
                provider: account.transport.provider.clone(),
                provider_kind: account.transport.provider_kind(),
                auth: account.transport.auth.clone(),
                base_url: account.transport.base_url.clone(),
                username: account.transport.username.clone(),
                imap: account.transport.imap.clone(),
                smtp: account.transport.smtp.clone(),
                secret,
            }
        }
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
            storage: SecretStorage::Os,
            configured: false,
            label: None,
        },
    }
}

fn default_account_appearance(account: &AccountSettings) -> AccountAppearance {
    AccountAppearance::Initials {
        initials: derive_account_initials(account),
        color_hue: account_color_hue(account),
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

fn derive_account_initials(account: &AccountSettings) -> String {
    let label = if account.name.trim().is_empty() {
        account.full_name.as_deref().unwrap_or("Account")
    } else {
        account.name.as_str()
    };
    normalize_initials(label)
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

fn account_color_hue(account: &AccountSettings) -> u16 {
    let seed = format!("{}:{}", account.id.as_str(), account.name);
    let hash = seed.bytes().fold(0_u32, |acc, byte| {
        acc.wrapping_mul(31).wrapping_add(byte as u32)
    });
    (hash % 361) as u16
}
