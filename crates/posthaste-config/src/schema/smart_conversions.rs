use super::*;

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
            role: self.role.clone(),
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
            role: mailbox.role.clone(),
            parent_id: mailbox.parent_id.as_ref().map(|id| id.to_string()),
            rule: convert_group_to_toml(&mailbox.rule.root),
            created_at: Some(mailbox.created_at.clone()),
            updated_at: Some(mailbox.updated_at.clone()),
        }
    }
}

/// Recursively converts a TOML rule group to the domain representation.
pub(crate) fn convert_rule_group(group: &RuleGroupToml) -> Result<SmartMailboxGroup, String> {
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
pub(crate) fn convert_rule_node(node: &RuleNodeToml) -> Result<SmartMailboxRuleNode, String> {
    match node {
        RuleNodeToml::Condition(condition) => Ok(SmartMailboxRuleNode::Condition(
            convert_condition(condition)?,
        )),
        RuleNodeToml::Group(group) => Ok(SmartMailboxRuleNode::Group(convert_rule_group(group)?)),
    }
}

/// Converts a TOML condition to the domain `SmartMailboxCondition`, mapping
/// field/operator enums and parsing the TOML value.
pub(crate) fn convert_condition(
    condition: &ConditionToml,
) -> Result<SmartMailboxCondition, String> {
    let field = match condition.field {
        FieldToml::SourceId => SmartMailboxField::SourceId,
        FieldToml::SourceName => SmartMailboxField::SourceName,
        FieldToml::MessageId => SmartMailboxField::MessageId,
        FieldToml::ThreadId => SmartMailboxField::ThreadId,
        FieldToml::ConversationId => SmartMailboxField::ConversationId,
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
pub(crate) fn convert_toml_value(value: &toml::Value) -> Result<SmartMailboxValue, String> {
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
pub(crate) fn convert_group_to_toml(group: &SmartMailboxGroup) -> RuleGroupToml {
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
pub(crate) fn convert_node_to_toml(node: &SmartMailboxRuleNode) -> RuleNodeToml {
    match node {
        SmartMailboxRuleNode::Condition(condition) => {
            RuleNodeToml::Condition(convert_condition_to_toml(condition))
        }
        SmartMailboxRuleNode::Group(group) => RuleNodeToml::Group(convert_group_to_toml(group)),
    }
}

/// Converts a domain condition back to its TOML representation.
pub(crate) fn convert_condition_to_toml(condition: &SmartMailboxCondition) -> ConditionToml {
    let field = match condition.field {
        SmartMailboxField::SourceId => FieldToml::SourceId,
        SmartMailboxField::SourceName => FieldToml::SourceName,
        SmartMailboxField::MessageId => FieldToml::MessageId,
        SmartMailboxField::ThreadId => FieldToml::ThreadId,
        SmartMailboxField::ConversationId => FieldToml::ConversationId,
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
pub(crate) fn convert_value_to_toml(value: &SmartMailboxValue) -> toml::Value {
    match value {
        SmartMailboxValue::String(s) => toml::Value::String(s.clone()),
        SmartMailboxValue::Bool(b) => toml::Value::Boolean(*b),
        SmartMailboxValue::Strings(arr) => {
            toml::Value::Array(arr.iter().map(|s| toml::Value::String(s.clone())).collect())
        }
    }
}
