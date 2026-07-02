use posthaste_domain_service::{
    AccountAppearance, AccountDriver, AccountId, AccountSettings, AccountTransportSettings,
    AppSettings, Appearance, AutomationAction, AutomationRule, AutomationTrigger, CachePolicy,
    GlassTheme, ImapTransportSettings, MailboxColor, MailboxId, Notifications, ProviderAuthKind,
    ProviderHint, SecretKind, SecretRef, SmartMailbox, SmartMailboxCondition, SmartMailboxField,
    SmartMailboxGroup, SmartMailboxGroupOperator, SmartMailboxId, SmartMailboxKind,
    SmartMailboxOperator, SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue,
    SmtpTransportSettings, TagAppearance, ThemeColors, ThemeMode, TransportSecurity, UiDensity,
    RFC3339_EPOCH,
};
use serde::{Deserialize, Serialize};

use crate::runtime::DaemonRuntimeTuning;

mod app;
mod automation;
mod enum_maps;
mod smart_conversions;
mod smart_types;
mod source_conversions;
mod source_types;

pub(crate) use app::AppToml;
#[cfg(test)]
use app::{CachePolicyToml, DaemonToml, LinkToml, LoggingToml};
pub use smart_types::{
    ConditionOperatorToml, ConditionToml, FieldToml, GroupOperatorToml, RuleGroupToml,
    RuleNodeToml, SmartMailboxKindToml, SmartMailboxToml,
};
pub use source_types::{
    AccountAppearanceToml, AutomationActionToml, AutomationRuleToml, AutomationTriggerToml,
    DriverToml, ImapTransportToml, ProviderAuthKindToml, ProviderHintToml, SecretKindToml,
    SecretRefToml, SmtpTransportToml, SourceToml, TransportSecurityToml, TransportToml,
};

use automation::{convert_automation_rule, convert_automation_rule_to_toml};
use smart_conversions::{convert_group_to_toml, convert_rule_group};

#[cfg(test)]
mod tests;
