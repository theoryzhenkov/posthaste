use posthaste_domain::{
    AccountAppearance, AccountDriver, AccountId, AccountSettings, AccountTransportSettings,
    AppSettings, AutomationAction, AutomationRule, AutomationTrigger, CachePolicy,
    ImapTransportSettings, MailboxId, ProviderAuthKind, ProviderHint, SecretKind, SecretRef,
    SmartMailbox, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
    SmartMailboxGroupOperator, SmartMailboxId, SmartMailboxKind, SmartMailboxOperator,
    SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue, SmtpTransportSettings,
    TransportSecurity, RFC3339_EPOCH,
};
use serde::{Deserialize, Serialize};

// -- app.toml --

/// TOML representation of the global `app.toml` config file.
///
/// @spec docs/L1-accounts#apptoml
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AppToml {
    #[serde(default)]
    pub schema_version: u32,
    pub default_source_id: Option<String>,
    #[serde(default)]
    pub automations: Vec<AutomationRuleToml>,
    #[serde(default)]
    pub draft_automations: Vec<AutomationRuleToml>,
    #[serde(default)]
    pub daemon: DaemonToml,
    #[serde(default)]
    pub logging: LoggingToml,
    #[serde(default)]
    pub cache: CachePolicyToml,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LoggingToml {
    pub level: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CachePolicyToml {
    pub soft_cap_bytes: Option<u64>,
    pub hard_cap_bytes: Option<u64>,
    pub cache_bodies: Option<bool>,
    pub cache_raw_messages: Option<bool>,
    pub cache_attachments: Option<bool>,
}

impl CachePolicyToml {
    fn to_cache_policy(&self) -> CachePolicy {
        let default = CachePolicy::default();
        CachePolicy {
            soft_cap_bytes: self.soft_cap_bytes.unwrap_or(default.soft_cap_bytes),
            hard_cap_bytes: self
                .hard_cap_bytes
                .unwrap_or(default.hard_cap_bytes)
                .max(self.soft_cap_bytes.unwrap_or(default.soft_cap_bytes)),
            cache_bodies: self.cache_bodies.unwrap_or(default.cache_bodies),
            cache_raw_messages: self
                .cache_raw_messages
                .unwrap_or(default.cache_raw_messages),
            cache_attachments: self.cache_attachments.unwrap_or(default.cache_attachments),
        }
    }

    fn from_cache_policy(policy: &CachePolicy) -> Self {
        Self {
            soft_cap_bytes: Some(policy.soft_cap_bytes),
            hard_cap_bytes: Some(policy.hard_cap_bytes),
            cache_bodies: Some(policy.cache_bodies),
            cache_raw_messages: Some(policy.cache_raw_messages),
            cache_attachments: Some(policy.cache_attachments),
        }
    }
}

/// Daemon-specific settings read only at startup (bind address, CORS, poll
/// interval).
///
/// @spec docs/L1-accounts#apptoml
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DaemonToml {
    pub bind: Option<String>,
    pub cors_origin: Option<String>,
    pub poll_interval_seconds: Option<u64>,
}

impl AppToml {
    /// Converts this TOML struct to the domain `AppSettings`.
    ///
    /// @spec docs/L1-accounts#toml-schema
    pub fn to_app_settings(&self) -> Result<AppSettings, String> {
        Ok(AppSettings {
            default_account_id: self.default_source_id.as_deref().map(AccountId::from),
            cache_policy: self.cache.to_cache_policy(),
            automation_rules: self
                .automations
                .iter()
                .map(convert_automation_rule)
                .collect::<Result<Vec<_>, _>>()?,
            automation_drafts: self
                .draft_automations
                .iter()
                .map(convert_automation_rule)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    /// Builds an `AppToml` from domain settings, preserving daemon config from
    /// the existing file.
    ///
    /// @spec docs/L1-accounts#toml-schema
    pub fn from_app_settings(settings: &AppSettings, existing: &AppToml) -> Self {
        Self {
            schema_version: existing.schema_version.max(1),
            default_source_id: settings
                .default_account_id
                .as_ref()
                .map(|id| id.to_string()),
            automations: settings
                .automation_rules
                .iter()
                .map(convert_automation_rule_to_toml)
                .collect(),
            draft_automations: settings
                .automation_drafts
                .iter()
                .map(convert_automation_rule_to_toml)
                .collect(),
            daemon: existing.daemon.clone(),
            logging: existing.logging.clone(),
            cache: CachePolicyToml::from_cache_policy(&settings.cache_policy),
        }
    }
}

// -- sources/<id>.toml --

/// TOML representation of an account source file (`sources/{id}.toml`).
///
/// @spec docs/L1-accounts#sourcesidtoml
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceToml {
    pub id: String,
    pub name: String,
    pub full_name: Option<String>,
    #[serde(default)]
    pub email_patterns: Vec<String>,
    pub driver: DriverToml,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub appearance: Option<AccountAppearanceToml>,
    #[serde(default)]
    pub transport: TransportToml,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Account driver variant: `jmap` or `mock`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverToml {
    Jmap,
    ImapSmtp,
    Mock,
}

/// TOML `[transport]` section: provider transport settings and credential
/// reference.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TransportToml {
    #[serde(default)]
    pub provider: ProviderHintToml,
    #[serde(default)]
    pub auth: ProviderAuthKindToml,
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub secret_ref: Option<SecretRefToml>,
    pub imap: Option<ImapTransportToml>,
    pub smtp: Option<SmtpTransportToml>,
}

/// TOML provider hint used for setup presets.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHintToml {
    #[default]
    Generic,
    Gmail,
    Outlook,
    Icloud,
}

/// TOML provider authentication kind.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAuthKindToml {
    #[default]
    Password,
    AppPassword,
    #[serde(rename = "oauth2")]
    OAuth2,
}

/// TOML TLS behavior for IMAP and SMTP endpoints.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportSecurityToml {
    #[default]
    Tls,
    StartTls,
    Plain,
}

/// TOML `[transport.imap]` section.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ImapTransportToml {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurityToml,
}

/// TOML `[transport.smtp]` section.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SmtpTransportToml {
    pub host: String,
    pub port: u16,
    pub security: TransportSecurityToml,
}

/// TOML `[appearance]` section for user-customizable account marks.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "snake_case",
    tag = "kind"
)]
pub enum AccountAppearanceToml {
    Initials {
        initials: String,
        color_hue: u16,
    },
    Image {
        image_id: String,
        initials: String,
        color_hue: u16,
    },
}

/// TOML `[[automations]]` item for global automation rules.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AutomationRuleToml {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub triggers: Vec<AutomationTriggerToml>,
    #[serde(default)]
    pub backfill: bool,
    pub condition: RuleGroupToml,
    #[serde(default)]
    pub actions: Vec<AutomationActionToml>,
}

/// TOML automation trigger.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationTriggerToml {
    MessageArrived,
    MessageChanged,
    Manual,
}

/// TOML automation action.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum AutomationActionToml {
    ApplyTag { tag: String },
    RemoveTag { tag: String },
    MarkRead,
    MarkUnread,
    Flag,
    Unflag,
    MoveToMailbox { mailbox_id: String },
}

/// Credential reference: OS keyring (`os`) or environment variable (`env`).
///
/// @spec docs/L0-accounts#credential-storage
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SecretRefToml {
    pub kind: SecretKindToml,
    pub key: String,
}

/// Secret storage backend: environment variable or OS keyring.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKindToml {
    Env,
    Os,
}

impl SourceToml {
    /// Converts this TOML struct to the domain `AccountSettings`. Missing
    /// timestamps default to `RFC3339_EPOCH`.
    ///
    /// @spec docs/L1-accounts#toml-schema
    pub fn to_account_settings(&self) -> Result<AccountSettings, String> {
        Ok(AccountSettings {
            id: AccountId::from(self.id.as_str()),
            name: self.name.clone(),
            full_name: self.full_name.clone(),
            email_patterns: self.email_patterns.clone(),
            driver: match self.driver {
                DriverToml::Jmap => AccountDriver::Jmap,
                DriverToml::ImapSmtp => AccountDriver::ImapSmtp,
                DriverToml::Mock => AccountDriver::Mock,
            },
            enabled: self.enabled,
            appearance: self.appearance.as_ref().map(|appearance| match appearance {
                AccountAppearanceToml::Initials {
                    initials,
                    color_hue,
                } => AccountAppearance::Initials {
                    initials: initials.clone(),
                    color_hue: *color_hue,
                },
                AccountAppearanceToml::Image {
                    image_id,
                    initials,
                    color_hue,
                } => AccountAppearance::Image {
                    image_id: image_id.clone(),
                    initials: initials.clone(),
                    color_hue: *color_hue,
                },
            }),
            transport: AccountTransportSettings {
                provider: convert_provider_hint(&self.transport.provider),
                auth: convert_auth_kind(&self.transport.auth),
                base_url: self.transport.base_url.clone(),
                username: self.transport.username.clone(),
                secret_ref: self.transport.secret_ref.as_ref().map(|sr| SecretRef {
                    kind: match sr.kind {
                        SecretKindToml::Env => SecretKind::Env,
                        SecretKindToml::Os => SecretKind::Os,
                    },
                    key: sr.key.clone(),
                }),
                imap: self
                    .transport
                    .imap
                    .as_ref()
                    .map(|imap| ImapTransportSettings {
                        host: imap.host.clone(),
                        port: imap.port,
                        security: convert_transport_security(&imap.security),
                    }),
                smtp: self
                    .transport
                    .smtp
                    .as_ref()
                    .map(|smtp| SmtpTransportSettings {
                        host: smtp.host.clone(),
                        port: smtp.port,
                        security: convert_transport_security(&smtp.security),
                    }),
            },
            created_at: self
                .created_at
                .clone()
                .unwrap_or_else(|| RFC3339_EPOCH.to_string()),
            updated_at: self
                .updated_at
                .clone()
                .unwrap_or_else(|| RFC3339_EPOCH.to_string()),
        })
    }

    /// Builds a `SourceToml` from domain `AccountSettings` for serialization.
    ///
    /// @spec docs/L1-accounts#toml-schema
    pub fn from_account_settings(settings: &AccountSettings) -> Self {
        Self {
            id: settings.id.to_string(),
            name: settings.name.clone(),
            full_name: settings.full_name.clone(),
            email_patterns: settings.email_patterns.clone(),
            driver: match settings.driver {
                AccountDriver::Jmap => DriverToml::Jmap,
                AccountDriver::ImapSmtp => DriverToml::ImapSmtp,
                AccountDriver::Mock => DriverToml::Mock,
            },
            enabled: settings.enabled,
            appearance: settings
                .appearance
                .as_ref()
                .map(|appearance| match appearance {
                    AccountAppearance::Initials {
                        initials,
                        color_hue,
                    } => AccountAppearanceToml::Initials {
                        initials: initials.clone(),
                        color_hue: *color_hue,
                    },
                    AccountAppearance::Image {
                        image_id,
                        initials,
                        color_hue,
                    } => AccountAppearanceToml::Image {
                        image_id: image_id.clone(),
                        initials: initials.clone(),
                        color_hue: *color_hue,
                    },
                }),
            transport: TransportToml {
                provider: convert_provider_hint_to_toml(&settings.transport.provider),
                auth: convert_auth_kind_to_toml(&settings.transport.auth),
                base_url: settings.transport.base_url.clone(),
                username: settings.transport.username.clone(),
                secret_ref: settings
                    .transport
                    .secret_ref
                    .as_ref()
                    .map(|sr| SecretRefToml {
                        kind: match sr.kind {
                            SecretKind::Env => SecretKindToml::Env,
                            SecretKind::Os => SecretKindToml::Os,
                        },
                        key: sr.key.clone(),
                    }),
                imap: settings
                    .transport
                    .imap
                    .as_ref()
                    .map(|imap| ImapTransportToml {
                        host: imap.host.clone(),
                        port: imap.port,
                        security: convert_transport_security_to_toml(&imap.security),
                    }),
                smtp: settings
                    .transport
                    .smtp
                    .as_ref()
                    .map(|smtp| SmtpTransportToml {
                        host: smtp.host.clone(),
                        port: smtp.port,
                        security: convert_transport_security_to_toml(&smtp.security),
                    }),
            },
            created_at: Some(settings.created_at.clone()),
            updated_at: Some(settings.updated_at.clone()),
        }
    }
}

fn convert_provider_hint(provider: &ProviderHintToml) -> ProviderHint {
    match provider {
        ProviderHintToml::Generic => ProviderHint::Generic,
        ProviderHintToml::Gmail => ProviderHint::Gmail,
        ProviderHintToml::Outlook => ProviderHint::Outlook,
        ProviderHintToml::Icloud => ProviderHint::Icloud,
    }
}

fn convert_provider_hint_to_toml(provider: &ProviderHint) -> ProviderHintToml {
    match provider {
        ProviderHint::Generic => ProviderHintToml::Generic,
        ProviderHint::Gmail => ProviderHintToml::Gmail,
        ProviderHint::Outlook => ProviderHintToml::Outlook,
        ProviderHint::Icloud => ProviderHintToml::Icloud,
    }
}

fn convert_auth_kind(auth: &ProviderAuthKindToml) -> ProviderAuthKind {
    match auth {
        ProviderAuthKindToml::Password => ProviderAuthKind::Password,
        ProviderAuthKindToml::AppPassword => ProviderAuthKind::AppPassword,
        ProviderAuthKindToml::OAuth2 => ProviderAuthKind::OAuth2,
    }
}

fn convert_auth_kind_to_toml(auth: &ProviderAuthKind) -> ProviderAuthKindToml {
    match auth {
        ProviderAuthKind::Password => ProviderAuthKindToml::Password,
        ProviderAuthKind::AppPassword => ProviderAuthKindToml::AppPassword,
        ProviderAuthKind::OAuth2 => ProviderAuthKindToml::OAuth2,
    }
}

fn convert_transport_security(security: &TransportSecurityToml) -> TransportSecurity {
    match security {
        TransportSecurityToml::Tls => TransportSecurity::Tls,
        TransportSecurityToml::StartTls => TransportSecurity::StartTls,
        TransportSecurityToml::Plain => TransportSecurity::Plain,
    }
}

fn convert_transport_security_to_toml(security: &TransportSecurity) -> TransportSecurityToml {
    match security {
        TransportSecurity::Tls => TransportSecurityToml::Tls,
        TransportSecurity::StartTls => TransportSecurityToml::StartTls,
        TransportSecurity::Plain => TransportSecurityToml::Plain,
    }
}

fn convert_automation_rule(rule: &AutomationRuleToml) -> Result<AutomationRule, String> {
    Ok(AutomationRule {
        id: rule.id.clone(),
        name: rule.name.clone(),
        enabled: rule.enabled,
        triggers: rule
            .triggers
            .iter()
            .map(convert_automation_trigger)
            .collect(),
        condition: SmartMailboxRule {
            root: convert_rule_group(&rule.condition)?,
        },
        actions: rule.actions.iter().map(convert_automation_action).collect(),
        backfill: rule.backfill,
    })
}

fn convert_automation_trigger(trigger: &AutomationTriggerToml) -> AutomationTrigger {
    match trigger {
        AutomationTriggerToml::MessageArrived => AutomationTrigger::MessageArrived,
        AutomationTriggerToml::MessageChanged => AutomationTrigger::MessageChanged,
        AutomationTriggerToml::Manual => AutomationTrigger::Manual,
    }
}

fn convert_automation_action(action: &AutomationActionToml) -> AutomationAction {
    match action {
        AutomationActionToml::ApplyTag { tag } => AutomationAction::ApplyTag { tag: tag.clone() },
        AutomationActionToml::RemoveTag { tag } => AutomationAction::RemoveTag { tag: tag.clone() },
        AutomationActionToml::MarkRead => AutomationAction::MarkRead,
        AutomationActionToml::MarkUnread => AutomationAction::MarkUnread,
        AutomationActionToml::Flag => AutomationAction::Flag,
        AutomationActionToml::Unflag => AutomationAction::Unflag,
        AutomationActionToml::MoveToMailbox { mailbox_id } => AutomationAction::MoveToMailbox {
            mailbox_id: MailboxId::from(mailbox_id.as_str()),
        },
    }
}

fn convert_automation_rule_to_toml(rule: &AutomationRule) -> AutomationRuleToml {
    AutomationRuleToml {
        id: rule.id.clone(),
        name: rule.name.clone(),
        enabled: rule.enabled,
        triggers: rule
            .triggers
            .iter()
            .map(convert_automation_trigger_to_toml)
            .collect(),
        backfill: rule.backfill,
        condition: convert_group_to_toml(&rule.condition.root),
        actions: rule
            .actions
            .iter()
            .map(convert_automation_action_to_toml)
            .collect(),
    }
}

fn convert_automation_trigger_to_toml(trigger: &AutomationTrigger) -> AutomationTriggerToml {
    match trigger {
        AutomationTrigger::MessageArrived => AutomationTriggerToml::MessageArrived,
        AutomationTrigger::MessageChanged => AutomationTriggerToml::MessageChanged,
        AutomationTrigger::Manual => AutomationTriggerToml::Manual,
    }
}

fn convert_automation_action_to_toml(action: &AutomationAction) -> AutomationActionToml {
    match action {
        AutomationAction::ApplyTag { tag } => AutomationActionToml::ApplyTag { tag: tag.clone() },
        AutomationAction::RemoveTag { tag } => AutomationActionToml::RemoveTag { tag: tag.clone() },
        AutomationAction::MarkRead => AutomationActionToml::MarkRead,
        AutomationAction::MarkUnread => AutomationActionToml::MarkUnread,
        AutomationAction::Flag => AutomationActionToml::Flag,
        AutomationAction::Unflag => AutomationActionToml::Unflag,
        AutomationAction::MoveToMailbox { mailbox_id } => AutomationActionToml::MoveToMailbox {
            mailbox_id: mailbox_id.to_string(),
        },
    }
}

// -- smart-mailboxes/<id>.toml --

/// TOML representation of a smart mailbox file (`smart-mailboxes/{id}.toml`).
/// Rules are recursive: groups contain nodes that are either leaf conditions or
/// nested groups.
///
/// @spec docs/L1-accounts#smart-mailboxesidtoml
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SmartMailboxToml {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub position: i64,
    #[serde(default = "default_user_kind")]
    pub kind: SmartMailboxKindToml,
    pub default_key: Option<String>,
    pub parent_id: Option<String>,
    pub rule: RuleGroupToml,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Whether a smart mailbox is a built-in default or user-created.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartMailboxKindToml {
    Default,
    User,
}

/// A group of rule nodes combined with a boolean operator (all/any), optionally
/// negated.
///
/// @spec docs/L1-accounts#smart-mailboxesidtoml
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuleGroupToml {
    #[serde(default = "default_all_operator")]
    pub operator: GroupOperatorToml,
    #[serde(default)]
    pub negated: bool,
    #[serde(default)]
    pub nodes: Vec<RuleNodeToml>,
}

/// Boolean group operator: `all` (AND) or `any` (OR).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupOperatorToml {
    All,
    Any,
}

/// A rule node: either a leaf `Condition` or a nested `Group`.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleNodeToml {
    Condition(ConditionToml),
    Group(RuleGroupToml),
}

/// A leaf condition matching a message field against a value with an operator.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConditionToml {
    pub field: FieldToml,
    pub operator: ConditionOperatorToml,
    #[serde(default)]
    pub negated: bool,
    pub value: toml::Value,
}

/// Message fields available for smart mailbox conditions.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldToml {
    SourceId,
    SourceName,
    MessageId,
    ThreadId,
    MailboxId,
    MailboxName,
    MailboxRole,
    IsRead,
    IsFlagged,
    HasAttachment,
    Keyword,
    FromName,
    FromEmail,
    Subject,
    Preview,
    ReceivedAt,
}

/// Comparison operators for smart mailbox conditions.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOperatorToml {
    Equals,
    In,
    Contains,
    Before,
    After,
    OnOrBefore,
    OnOrAfter,
}

// -- Conversions --

impl SmartMailboxToml {
    /// Converts this TOML struct to the domain `SmartMailbox`, recursively
    /// converting the rule tree.
    ///
    /// @spec docs/L1-accounts#toml-schema
    pub fn to_smart_mailbox(&self) -> Result<SmartMailbox, String> {
        Ok(SmartMailbox {
            id: SmartMailboxId::from(self.id.as_str()),
            name: self.name.clone(),
            position: self.position,
            kind: match self.kind {
                SmartMailboxKindToml::Default => SmartMailboxKind::Default,
                SmartMailboxKindToml::User => SmartMailboxKind::User,
            },
            default_key: self.default_key.clone(),
            parent_id: self.parent_id.as_deref().map(SmartMailboxId::from),
            rule: SmartMailboxRule {
                root: convert_rule_group(&self.rule)?,
            },
            created_at: self
                .created_at
                .clone()
                .unwrap_or_else(|| RFC3339_EPOCH.to_string()),
            updated_at: self
                .updated_at
                .clone()
                .unwrap_or_else(|| RFC3339_EPOCH.to_string()),
        })
    }

    /// Builds a `SmartMailboxToml` from a domain `SmartMailbox` for
    /// serialization.
    ///
    /// @spec docs/L1-accounts#toml-schema
    pub fn from_smart_mailbox(mailbox: &SmartMailbox) -> Self {
        Self {
            id: mailbox.id.to_string(),
            name: mailbox.name.clone(),
            position: mailbox.position,
            kind: match mailbox.kind {
                SmartMailboxKind::Default => SmartMailboxKindToml::Default,
                SmartMailboxKind::User => SmartMailboxKindToml::User,
            },
            default_key: mailbox.default_key.clone(),
            parent_id: mailbox.parent_id.as_ref().map(|id| id.to_string()),
            rule: convert_group_to_toml(&mailbox.rule.root),
            created_at: Some(mailbox.created_at.clone()),
            updated_at: Some(mailbox.updated_at.clone()),
        }
    }
}

/// Recursively converts a TOML rule group to the domain representation.
fn convert_rule_group(group: &RuleGroupToml) -> Result<SmartMailboxGroup, String> {
    let operator = match group.operator {
        GroupOperatorToml::All => SmartMailboxGroupOperator::All,
        GroupOperatorToml::Any => SmartMailboxGroupOperator::Any,
    };
    let nodes = group
        .nodes
        .iter()
        .map(convert_rule_node)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(SmartMailboxGroup {
        operator,
        negated: group.negated,
        nodes,
    })
}

/// Converts a single TOML rule node (condition or group) to the domain type.
fn convert_rule_node(node: &RuleNodeToml) -> Result<SmartMailboxRuleNode, String> {
    match node {
        RuleNodeToml::Condition(condition) => Ok(SmartMailboxRuleNode::Condition(
            convert_condition(condition)?,
        )),
        RuleNodeToml::Group(group) => Ok(SmartMailboxRuleNode::Group(convert_rule_group(group)?)),
    }
}

/// Converts a TOML condition to the domain `SmartMailboxCondition`, mapping
/// field/operator enums and parsing the TOML value.
fn convert_condition(condition: &ConditionToml) -> Result<SmartMailboxCondition, String> {
    let field = match condition.field {
        FieldToml::SourceId => SmartMailboxField::SourceId,
        FieldToml::SourceName => SmartMailboxField::SourceName,
        FieldToml::MessageId => SmartMailboxField::MessageId,
        FieldToml::ThreadId => SmartMailboxField::ThreadId,
        FieldToml::MailboxId => SmartMailboxField::MailboxId,
        FieldToml::MailboxName => SmartMailboxField::MailboxName,
        FieldToml::MailboxRole => SmartMailboxField::MailboxRole,
        FieldToml::IsRead => SmartMailboxField::IsRead,
        FieldToml::IsFlagged => SmartMailboxField::IsFlagged,
        FieldToml::HasAttachment => SmartMailboxField::HasAttachment,
        FieldToml::Keyword => SmartMailboxField::Keyword,
        FieldToml::FromName => SmartMailboxField::FromName,
        FieldToml::FromEmail => SmartMailboxField::FromEmail,
        FieldToml::Subject => SmartMailboxField::Subject,
        FieldToml::Preview => SmartMailboxField::Preview,
        FieldToml::ReceivedAt => SmartMailboxField::ReceivedAt,
    };
    let operator = match condition.operator {
        ConditionOperatorToml::Equals => SmartMailboxOperator::Equals,
        ConditionOperatorToml::In => SmartMailboxOperator::In,
        ConditionOperatorToml::Contains => SmartMailboxOperator::Contains,
        ConditionOperatorToml::Before => SmartMailboxOperator::Before,
        ConditionOperatorToml::After => SmartMailboxOperator::After,
        ConditionOperatorToml::OnOrBefore => SmartMailboxOperator::OnOrBefore,
        ConditionOperatorToml::OnOrAfter => SmartMailboxOperator::OnOrAfter,
    };
    let value = convert_toml_value(&condition.value)?;
    Ok(SmartMailboxCondition {
        field,
        operator,
        negated: condition.negated,
        value,
    })
}

/// Converts a TOML value to a `SmartMailboxValue`. Supports string, boolean,
/// and string arrays (for `in` operator).
fn convert_toml_value(value: &toml::Value) -> Result<SmartMailboxValue, String> {
    match value {
        toml::Value::String(s) => Ok(SmartMailboxValue::String(s.clone())),
        toml::Value::Boolean(b) => Ok(SmartMailboxValue::Bool(*b)),
        toml::Value::Array(arr) => {
            let strings: Result<Vec<String>, _> = arr
                .iter()
                .map(|v| match v {
                    toml::Value::String(s) => Ok(s.clone()),
                    _ => Err("array values must be strings".to_string()),
                })
                .collect();
            Ok(SmartMailboxValue::Strings(strings?))
        }
        _ => Err(format!("unsupported TOML value type: {value}")),
    }
}

// -- Domain → TOML conversions --

/// Recursively converts a domain rule group back to the TOML representation.
fn convert_group_to_toml(group: &SmartMailboxGroup) -> RuleGroupToml {
    RuleGroupToml {
        operator: match group.operator {
            SmartMailboxGroupOperator::All => GroupOperatorToml::All,
            SmartMailboxGroupOperator::Any => GroupOperatorToml::Any,
        },
        negated: group.negated,
        nodes: group.nodes.iter().map(convert_node_to_toml).collect(),
    }
}

/// Converts a single domain rule node back to TOML.
fn convert_node_to_toml(node: &SmartMailboxRuleNode) -> RuleNodeToml {
    match node {
        SmartMailboxRuleNode::Condition(condition) => {
            RuleNodeToml::Condition(convert_condition_to_toml(condition))
        }
        SmartMailboxRuleNode::Group(group) => RuleNodeToml::Group(convert_group_to_toml(group)),
    }
}

/// Converts a domain condition back to its TOML representation.
fn convert_condition_to_toml(condition: &SmartMailboxCondition) -> ConditionToml {
    let field = match condition.field {
        SmartMailboxField::SourceId => FieldToml::SourceId,
        SmartMailboxField::SourceName => FieldToml::SourceName,
        SmartMailboxField::MessageId => FieldToml::MessageId,
        SmartMailboxField::ThreadId => FieldToml::ThreadId,
        SmartMailboxField::MailboxId => FieldToml::MailboxId,
        SmartMailboxField::MailboxName => FieldToml::MailboxName,
        SmartMailboxField::MailboxRole => FieldToml::MailboxRole,
        SmartMailboxField::IsRead => FieldToml::IsRead,
        SmartMailboxField::IsFlagged => FieldToml::IsFlagged,
        SmartMailboxField::HasAttachment => FieldToml::HasAttachment,
        SmartMailboxField::Keyword => FieldToml::Keyword,
        SmartMailboxField::FromName => FieldToml::FromName,
        SmartMailboxField::FromEmail => FieldToml::FromEmail,
        SmartMailboxField::Subject => FieldToml::Subject,
        SmartMailboxField::Preview => FieldToml::Preview,
        SmartMailboxField::ReceivedAt => FieldToml::ReceivedAt,
    };
    let operator = match condition.operator {
        SmartMailboxOperator::Equals => ConditionOperatorToml::Equals,
        SmartMailboxOperator::In => ConditionOperatorToml::In,
        SmartMailboxOperator::Contains => ConditionOperatorToml::Contains,
        SmartMailboxOperator::Before => ConditionOperatorToml::Before,
        SmartMailboxOperator::After => ConditionOperatorToml::After,
        SmartMailboxOperator::OnOrBefore => ConditionOperatorToml::OnOrBefore,
        SmartMailboxOperator::OnOrAfter => ConditionOperatorToml::OnOrAfter,
    };
    let value = convert_value_to_toml(&condition.value);
    ConditionToml {
        field,
        operator,
        negated: condition.negated,
        value,
    }
}

/// Converts a domain `SmartMailboxValue` back to a `toml::Value`.
fn convert_value_to_toml(value: &SmartMailboxValue) -> toml::Value {
    match value {
        SmartMailboxValue::String(s) => toml::Value::String(s.clone()),
        SmartMailboxValue::Bool(b) => toml::Value::Boolean(*b),
        SmartMailboxValue::Strings(arr) => {
            toml::Value::Array(arr.iter().map(|s| toml::Value::String(s.clone())).collect())
        }
    }
}

// -- Helpers --

/// Serde default for `SourceToml.enabled`.
fn default_true() -> bool {
    true
}

/// Serde default: smart mailboxes default to user-created kind.
fn default_user_kind() -> SmartMailboxKindToml {
    SmartMailboxKindToml::User
}

/// Serde default: rule groups default to the `All` (AND) operator.
fn default_all_operator() -> GroupOperatorToml {
    GroupOperatorToml::All
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::defaults::default_smart_mailboxes;

    #[test]
    fn source_toml_round_trips() {
        let settings = AccountSettings {
            id: AccountId::from("primary"),
            name: "My Fastmail".to_string(),
            full_name: Some("Example User".to_string()),
            email_patterns: vec!["user@example.com".to_string(), "*@example.net".to_string()],
            driver: AccountDriver::Jmap,
            enabled: true,
            appearance: Some(AccountAppearance::Initials {
                initials: "MF".to_string(),
                color_hue: 245,
            }),
            transport: AccountTransportSettings {
                base_url: Some("https://api.fastmail.com".to_string()),
                username: Some("user@example.com".to_string()),
                secret_ref: Some(SecretRef {
                    kind: SecretKind::Os,
                    key: "account:primary".to_string(),
                }),
                ..Default::default()
            },
            created_at: "2026-03-31T00:00:00Z".to_string(),
            updated_at: "2026-03-31T00:00:00Z".to_string(),
        };

        let toml_struct = SourceToml::from_account_settings(&settings);
        let toml_string = toml::to_string_pretty(&toml_struct).unwrap();
        let parsed: SourceToml = toml::from_str(&toml_string).unwrap();
        let round_tripped = parsed.to_account_settings().unwrap();

        assert_eq!(round_tripped, settings);
    }

    #[test]
    fn imap_smtp_source_toml_round_trips_provider_transport() {
        let settings = AccountSettings {
            id: AccountId::from("icloud"),
            name: "iCloud".to_string(),
            full_name: None,
            email_patterns: vec!["user@icloud.com".to_string()],
            driver: AccountDriver::ImapSmtp,
            enabled: true,
            appearance: None,
            transport: AccountTransportSettings {
                provider: ProviderHint::Icloud,
                auth: ProviderAuthKind::AppPassword,
                username: Some("user@icloud.com".to_string()),
                secret_ref: Some(SecretRef {
                    kind: SecretKind::Os,
                    key: "account:icloud".to_string(),
                }),
                imap: Some(ImapTransportSettings {
                    host: "imap.mail.me.com".to_string(),
                    port: 993,
                    security: TransportSecurity::Tls,
                }),
                smtp: Some(SmtpTransportSettings {
                    host: "smtp.mail.me.com".to_string(),
                    port: 587,
                    security: TransportSecurity::StartTls,
                }),
                ..Default::default()
            },
            created_at: "2026-04-25T00:00:00Z".to_string(),
            updated_at: "2026-04-25T00:00:00Z".to_string(),
        };

        let toml_struct = SourceToml::from_account_settings(&settings);
        let toml_string = toml::to_string_pretty(&toml_struct).unwrap();
        assert!(toml_string.contains("driver = \"imap_smtp\""));
        assert!(toml_string.contains("provider = \"icloud\""));

        let parsed: SourceToml = toml::from_str(&toml_string).unwrap();
        let round_tripped = parsed.to_account_settings().unwrap();

        assert_eq!(round_tripped, settings);
    }

    #[test]
    fn default_smart_mailboxes_round_trip_through_toml() {
        for mailbox in default_smart_mailboxes() {
            let toml_struct = SmartMailboxToml::from_smart_mailbox(&mailbox);
            let toml_string = toml::to_string_pretty(&toml_struct).unwrap();
            let parsed: SmartMailboxToml = toml::from_str(&toml_string).unwrap();
            let round_tripped = parsed.to_smart_mailbox().unwrap();

            assert_eq!(round_tripped.id, mailbox.id);
            assert_eq!(round_tripped.name, mailbox.name);
            assert_eq!(round_tripped.kind, mailbox.kind);
            assert_eq!(round_tripped.default_key, mailbox.default_key);
            assert_eq!(round_tripped.rule, mailbox.rule);
        }
    }

    #[test]
    fn app_toml_round_trips() {
        let settings = AppSettings {
            default_account_id: Some(AccountId::from("primary")),
            automation_rules: vec![AutomationRule {
                id: "rule-newsletters".to_string(),
                name: "Newsletters".to_string(),
                enabled: true,
                triggers: vec![AutomationTrigger::MessageArrived],
                condition: SmartMailboxRule {
                    root: SmartMailboxGroup {
                        operator: SmartMailboxGroupOperator::Any,
                        negated: false,
                        nodes: vec![
                            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                                field: SmartMailboxField::FromName,
                                operator: SmartMailboxOperator::Contains,
                                negated: false,
                                value: SmartMailboxValue::String("Posthaste".to_string()),
                            }),
                            SmartMailboxRuleNode::Condition(SmartMailboxCondition {
                                field: SmartMailboxField::FromEmail,
                                operator: SmartMailboxOperator::Contains,
                                negated: false,
                                value: SmartMailboxValue::String("Posthaste".to_string()),
                            }),
                        ],
                    },
                },
                actions: vec![AutomationAction::ApplyTag {
                    tag: "newsletter".to_string(),
                }],
                backfill: true,
            }],
            automation_drafts: vec![AutomationRule {
                id: "draft-newsletters".to_string(),
                name: "Draft newsletters".to_string(),
                enabled: true,
                triggers: vec![AutomationTrigger::MessageArrived],
                condition: SmartMailboxRule {
                    root: SmartMailboxGroup {
                        operator: SmartMailboxGroupOperator::Any,
                        negated: false,
                        nodes: Vec::new(),
                    },
                },
                actions: vec![AutomationAction::ApplyTag { tag: String::new() }],
                backfill: true,
            }],
            ..Default::default()
        };
        let existing = AppToml {
            schema_version: 1,
            default_source_id: None,
            automations: Vec::new(),
            draft_automations: Vec::new(),
            daemon: DaemonToml::default(),
            logging: LoggingToml::default(),
            cache: CachePolicyToml::default(),
        };
        let toml_struct = AppToml::from_app_settings(&settings, &existing);
        let toml_string = toml::to_string_pretty(&toml_struct).unwrap();
        let parsed: AppToml = toml::from_str(&toml_string).unwrap();
        let round_tripped = parsed.to_app_settings().unwrap();

        assert_eq!(round_tripped, settings);
    }
}
