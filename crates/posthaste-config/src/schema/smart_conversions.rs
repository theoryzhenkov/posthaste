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
            kind: self.kind.to_domain(),
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
            kind: SmartMailboxKindToml::from_domain(&mailbox.kind),
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
    let operator = group.operator.to_domain();
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
    let field = condition.field.to_domain();
    let operator = condition.operator.to_domain();
    let value = convert_toml_value(&condition.value)?;
    Ok(SmartMailboxCondition {
        field,
        operator,
        negated: condition.negated,
        value,
    })
}

/// Converts a TOML value to a `SmartMailboxValue`. Supports string, boolean,
/// string arrays (for the `in` operator), and a typed date table (a `[table]`
/// with a `kind` discriminator — `absolute`/`relative` — mirroring the
/// [`DateValue`] wire shape). Legacy bare-string date values still load as a
/// plain `String`, so no migration is required.
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
        toml::Value::Table(table) => convert_toml_date_table(table),
        _ => Err(format!("unsupported TOML value type: {value}")),
    }
}

/// Converts a TOML date table (`{ kind = "absolute"/"relative", ... }`) to a
/// [`SmartMailboxValue::Date`].
fn convert_toml_date_table(table: &toml::value::Table) -> Result<SmartMailboxValue, String> {
    let kind = table
        .get("kind")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| "date value table must have a string `kind`".to_string())?;
    let date = match kind {
        "absolute" => {
            let value = table
                .get("value")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| "absolute date value must have a string `value`".to_string())?;
            DateValue::Absolute {
                value: value.to_string(),
            }
        }
        "relative" => {
            let amount = table
                .get("amount")
                .and_then(toml::Value::as_integer)
                .ok_or_else(|| "relative date value must have an integer `amount`".to_string())?;
            let amount = u32::try_from(amount)
                .map_err(|_| "relative date `amount` out of range".to_string())?;
            let unit = table
                .get("unit")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| "relative date value must have a string `unit`".to_string())?;
            DateValue::Relative {
                amount,
                unit: date_unit_from_str(unit)?,
            }
        }
        other => return Err(format!("unknown date value kind: {other}")),
    };
    Ok(SmartMailboxValue::Date(date))
}

/// Maps a TOML/wire `unit` string to a [`DateUnit`].
fn date_unit_from_str(unit: &str) -> Result<DateUnit, String> {
    match unit {
        "minutes" => Ok(DateUnit::Minutes),
        "hours" => Ok(DateUnit::Hours),
        "days" => Ok(DateUnit::Days),
        "weeks" => Ok(DateUnit::Weeks),
        "months" => Ok(DateUnit::Months),
        other => Err(format!("unknown relative date unit: {other}")),
    }
}

/// The wire/TOML `unit` string for a [`DateUnit`].
fn date_unit_str(unit: &DateUnit) -> &'static str {
    match unit {
        DateUnit::Minutes => "minutes",
        DateUnit::Hours => "hours",
        DateUnit::Days => "days",
        DateUnit::Weeks => "weeks",
        DateUnit::Months => "months",
    }
}

// -- Domain → TOML conversions --

/// Recursively converts a domain rule group back to the TOML representation.
pub(crate) fn convert_group_to_toml(group: &SmartMailboxGroup) -> RuleGroupToml {
    RuleGroupToml {
        operator: GroupOperatorToml::from_domain(&group.operator),
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
    let field = FieldToml::from_domain(&condition.field);
    let operator = ConditionOperatorToml::from_domain(&condition.operator);
    let value = convert_value_to_toml(&condition.value);
    ConditionToml {
        field,
        operator,
        negated: condition.negated,
        value,
    }
}

/// Converts a domain `SmartMailboxValue` back to a `toml::Value`. A `Date`
/// serializes to a `{ kind = ..., ... }` table (round-tripping
/// [`convert_toml_date_table`]).
pub(crate) fn convert_value_to_toml(value: &SmartMailboxValue) -> toml::Value {
    match value {
        SmartMailboxValue::String(s) => toml::Value::String(s.clone()),
        SmartMailboxValue::Bool(b) => toml::Value::Boolean(*b),
        SmartMailboxValue::Strings(arr) => {
            toml::Value::Array(arr.iter().map(|s| toml::Value::String(s.clone())).collect())
        }
        SmartMailboxValue::Date(date) => {
            let mut table = toml::value::Table::new();
            match date {
                DateValue::Absolute { value } => {
                    table.insert(
                        "kind".to_string(),
                        toml::Value::String("absolute".to_string()),
                    );
                    table.insert("value".to_string(), toml::Value::String(value.clone()));
                }
                DateValue::Relative { amount, unit } => {
                    table.insert(
                        "kind".to_string(),
                        toml::Value::String("relative".to_string()),
                    );
                    table.insert(
                        "amount".to_string(),
                        toml::Value::Integer(i64::from(*amount)),
                    );
                    table.insert(
                        "unit".to_string(),
                        toml::Value::String(date_unit_str(unit).to_string()),
                    );
                }
            }
            toml::Value::Table(table)
        }
    }
}
