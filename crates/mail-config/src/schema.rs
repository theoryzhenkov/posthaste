use mail_domain::{
    AccountDriver, AccountId, AccountSettings, AccountTransportSettings, AppSettings, SecretKind,
    SecretRef, SmartMailbox, SmartMailboxCondition, SmartMailboxField, SmartMailboxGroup,
    SmartMailboxGroupOperator, SmartMailboxId, SmartMailboxKind, SmartMailboxOperator,
    SmartMailboxRule, SmartMailboxRuleNode, SmartMailboxValue,
};
use serde::{Deserialize, Serialize};

// -- app.toml --

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AppToml {
    #[serde(default)]
    pub schema_version: u32,
    pub default_source_id: Option<String>,
    #[serde(default)]
    pub daemon: DaemonToml,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DaemonToml {
    pub bind: Option<String>,
    pub cors_origin: Option<String>,
    pub poll_interval_seconds: Option<u64>,
}

impl Default for DaemonToml {
    fn default() -> Self {
        Self {
            bind: None,
            cors_origin: None,
            poll_interval_seconds: None,
        }
    }
}

impl AppToml {
    pub fn to_app_settings(&self) -> AppSettings {
        AppSettings {
            default_account_id: self.default_source_id.as_deref().map(AccountId::from),
        }
    }

    pub fn from_app_settings(settings: &AppSettings, existing: &AppToml) -> Self {
        Self {
            schema_version: existing.schema_version.max(1),
            default_source_id: settings
                .default_account_id
                .as_ref()
                .map(|id| id.to_string()),
            daemon: existing.daemon.clone(),
        }
    }
}

// -- sources/<id>.toml --

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceToml {
    pub id: String,
    pub name: String,
    pub driver: DriverToml,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub transport: TransportToml,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverToml {
    Jmap,
    Mock,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TransportToml {
    pub base_url: Option<String>,
    pub username: Option<String>,
    pub secret_ref: Option<SecretRefToml>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SecretRefToml {
    pub kind: SecretKindToml,
    pub key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKindToml {
    Env,
    Os,
}

impl SourceToml {
    pub fn to_account_settings(&self) -> AccountSettings {
        AccountSettings {
            id: AccountId::from(self.id.as_str()),
            name: self.name.clone(),
            driver: match self.driver {
                DriverToml::Jmap => AccountDriver::Jmap,
                DriverToml::Mock => AccountDriver::Mock,
            },
            enabled: self.enabled,
            transport: AccountTransportSettings {
                base_url: self.transport.base_url.clone(),
                username: self.transport.username.clone(),
                secret_ref: self.transport.secret_ref.as_ref().map(|sr| SecretRef {
                    kind: match sr.kind {
                        SecretKindToml::Env => SecretKind::Env,
                        SecretKindToml::Os => SecretKind::Os,
                    },
                    key: sr.key.clone(),
                }),
            },
            created_at: self
                .created_at
                .clone()
                .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string()),
            updated_at: self
                .updated_at
                .clone()
                .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string()),
        }
    }

    pub fn from_account_settings(settings: &AccountSettings) -> Self {
        Self {
            id: settings.id.to_string(),
            name: settings.name.clone(),
            driver: match settings.driver {
                AccountDriver::Jmap => DriverToml::Jmap,
                AccountDriver::Mock => DriverToml::Mock,
            },
            enabled: settings.enabled,
            transport: TransportToml {
                base_url: settings.transport.base_url.clone(),
                username: settings.transport.username.clone(),
                secret_ref: settings.transport.secret_ref.as_ref().map(|sr| {
                    SecretRefToml {
                        kind: match sr.kind {
                            SecretKind::Env => SecretKindToml::Env,
                            SecretKind::Os => SecretKindToml::Os,
                        },
                        key: sr.key.clone(),
                    }
                }),
            },
            created_at: Some(settings.created_at.clone()),
            updated_at: Some(settings.updated_at.clone()),
        }
    }
}

// -- smart-mailboxes/<id>.toml --

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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SmartMailboxKindToml {
    Default,
    User,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RuleGroupToml {
    #[serde(default = "default_all_operator")]
    pub operator: GroupOperatorToml,
    #[serde(default)]
    pub negated: bool,
    #[serde(default)]
    pub nodes: Vec<RuleNodeToml>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupOperatorToml {
    All,
    Any,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RuleNodeToml {
    Condition(ConditionToml),
    Group(RuleGroupToml),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConditionToml {
    pub field: FieldToml,
    pub operator: ConditionOperatorToml,
    #[serde(default)]
    pub negated: bool,
    pub value: toml::Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldToml {
    SourceId,
    SourceName,
    MailboxId,
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
                .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string()),
            updated_at: self
                .updated_at
                .clone()
                .unwrap_or_else(|| "1970-01-01T00:00:00Z".to_string()),
        })
    }

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

fn convert_rule_node(node: &RuleNodeToml) -> Result<SmartMailboxRuleNode, String> {
    match node {
        RuleNodeToml::Condition(condition) => {
            Ok(SmartMailboxRuleNode::Condition(convert_condition(condition)?))
        }
        RuleNodeToml::Group(group) => {
            Ok(SmartMailboxRuleNode::Group(convert_rule_group(group)?))
        }
    }
}

fn convert_condition(condition: &ConditionToml) -> Result<SmartMailboxCondition, String> {
    let field = match condition.field {
        FieldToml::SourceId => SmartMailboxField::SourceId,
        FieldToml::SourceName => SmartMailboxField::SourceName,
        FieldToml::MailboxId => SmartMailboxField::MailboxId,
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

fn convert_node_to_toml(node: &SmartMailboxRuleNode) -> RuleNodeToml {
    match node {
        SmartMailboxRuleNode::Condition(condition) => {
            RuleNodeToml::Condition(convert_condition_to_toml(condition))
        }
        SmartMailboxRuleNode::Group(group) => {
            RuleNodeToml::Group(convert_group_to_toml(group))
        }
    }
}

fn convert_condition_to_toml(condition: &SmartMailboxCondition) -> ConditionToml {
    let field = match condition.field {
        SmartMailboxField::SourceId => FieldToml::SourceId,
        SmartMailboxField::SourceName => FieldToml::SourceName,
        SmartMailboxField::MailboxId => FieldToml::MailboxId,
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

fn default_true() -> bool {
    true
}

fn default_user_kind() -> SmartMailboxKindToml {
    SmartMailboxKindToml::User
}

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
            driver: AccountDriver::Jmap,
            enabled: true,
            transport: AccountTransportSettings {
                base_url: Some("https://api.fastmail.com".to_string()),
                username: Some("user@example.com".to_string()),
                secret_ref: Some(SecretRef {
                    kind: SecretKind::Os,
                    key: "account:primary".to_string(),
                }),
            },
            created_at: "2026-03-31T00:00:00Z".to_string(),
            updated_at: "2026-03-31T00:00:00Z".to_string(),
        };

        let toml_struct = SourceToml::from_account_settings(&settings);
        let toml_string = toml::to_string_pretty(&toml_struct).unwrap();
        let parsed: SourceToml = toml::from_str(&toml_string).unwrap();
        let round_tripped = parsed.to_account_settings();

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
        };
        let existing = AppToml {
            schema_version: 1,
            default_source_id: None,
            daemon: DaemonToml::default(),
        };
        let toml_struct = AppToml::from_app_settings(&settings, &existing);
        let toml_string = toml::to_string_pretty(&toml_struct).unwrap();
        let parsed: AppToml = toml::from_str(&toml_string).unwrap();
        let round_tripped = parsed.to_app_settings();

        assert_eq!(round_tripped, settings);
    }
}
