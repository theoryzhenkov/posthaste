use super::*;

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
    #[serde(default = "default_user_kind")]
    pub kind: SmartMailboxKindToml,
    pub default_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
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
    ConversationId,
    MailboxId,
    MailboxName,
    MailboxRole,
    IsRead,
    IsFlagged,
    HasAttachment,
    Keyword,
    FromName,
    FromEmail,
    To,
    Subject,
    Preview,
    /// Full-text match over the cached message body (FTS-backed `contains`).
    Body,
    ReceivedAt,
    Size,
}

/// Comparison operators for smart mailbox conditions.
///
/// The four ordered comparisons are the neutral `Lt`/`Gt`/`Le`/`Ge` (D6). Stored
/// TOML written before the rename used the old snake_case names
/// (`before`/`after`/`on_or_before`/`on_or_after`); the `#[serde(alias = ...)]`s
/// keep those readable, while serialization emits the new `lt`/`gt`/`le`/`ge`.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOperatorToml {
    Equals,
    In,
    Contains,
    /// Prefix match (`begins_with`) — additive text operator (R4).
    BeginsWith,
    /// Suffix match (`ends_with`) — additive text operator (R4).
    EndsWith,
    /// Regex match (`regex`) — additive text operator (R4).
    Regex,
    #[serde(alias = "before")]
    Lt,
    #[serde(alias = "after")]
    Gt,
    #[serde(alias = "on_or_before")]
    Le,
    #[serde(alias = "on_or_after")]
    Ge,
}

// -- Helpers --

/// Serde default: smart mailboxes default to user-created kind.
fn default_user_kind() -> SmartMailboxKindToml {
    SmartMailboxKindToml::User
}

/// Serde default: rule groups default to the `All` (AND) operator.
fn default_all_operator() -> GroupOperatorToml {
    GroupOperatorToml::All
}
