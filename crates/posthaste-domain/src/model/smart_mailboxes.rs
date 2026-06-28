use super::*;

/// Distinguishes built-in smart mailboxes from user-created ones.
///
/// @spec docs/L1-accounts#smart-mailbox-defaults
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxKind {
    Default,
    User,
}

/// User-facing tag derived from non-system JMAP keywords.
///
/// @spec docs/L1-api#navigation
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TagSummary {
    pub name: String,
    pub unread_messages: i64,
    pub total_messages: i64,
}

/// Boolean combinator for smart mailbox rule groups: `All` (AND) or `Any` (OR).
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxGroupOperator {
    All,
    Any,
}

/// Message field that a smart mailbox condition can filter on.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxField {
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
    Subject,
    Preview,
    ReceivedAt,
}

/// Comparison operator for a smart mailbox condition.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxOperator {
    Equals,
    In,
    Contains,
    Before,
    After,
    OnOrBefore,
    OnOrAfter,
}

/// Condition value: scalar string, string list (for `In`), or boolean.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxValue {
    String(String),
    Strings(Vec<String>),
    Bool(bool),
}

/// Boolean group node containing child conditions or nested groups.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmartMailboxGroup {
    pub operator: SmartMailboxGroupOperator,
    pub negated: bool,
    // Break the SmartMailboxGroup -> SmartMailboxRuleNode -> SmartMailboxGroup
    // schema cycle so utoipa's component collector does not recurse infinitely.
    // The emitted schema still references SmartMailboxRuleNode by `$ref`.
    #[cfg_attr(feature = "openapi", schema(no_recursion))]
    pub nodes: Vec<SmartMailboxRuleNode>,
}

/// Leaf condition matching a single field with an operator and value.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmartMailboxCondition {
    pub field: SmartMailboxField,
    pub operator: SmartMailboxOperator,
    pub negated: bool,
    pub value: SmartMailboxValue,
}

/// Recursive rule tree node: either a [`SmartMailboxGroup`] or a [`SmartMailboxCondition`].
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxRuleNode {
    Group(SmartMailboxGroup),
    Condition(SmartMailboxCondition),
}

/// Top-level rule for a smart mailbox, wrapping a root group.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmartMailboxRule {
    pub root: SmartMailboxGroup,
}

/// A saved query with display metadata that behaves like a virtual mailbox.
///
/// @spec docs/L0-search#smart-mailboxes
/// @spec docs/L1-accounts#smart-mailbox-defaults
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmartMailbox {
    pub id: SmartMailboxId,
    pub name: String,
    pub position: i64,
    pub kind: SmartMailboxKind,
    /// Identifies built-in smart mailboxes (e.g. "inbox", "trash").
    pub default_key: Option<String>,
    /// The mailbox role whose semantics apply to this view (e.g. `"trash"`),
    /// driving contextual actions like Delete Permanently. Set on the built-in
    /// role defaults; `None` for All Mail and unassigned user smart mailboxes.
    pub role: Option<String>,
    pub parent_id: Option<SmartMailboxId>,
    pub rule: SmartMailboxRule,
    pub created_at: String,
    pub updated_at: String,
}

/// Smart mailbox config with live unread/total counts from the store.
///
/// @spec docs/L1-api#smart-mailboxes
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SmartMailboxSummary {
    pub id: SmartMailboxId,
    pub name: String,
    pub position: i64,
    pub kind: SmartMailboxKind,
    pub default_key: Option<String>,
    pub role: Option<String>,
    pub parent_id: Option<SmartMailboxId>,
    pub unread_messages: i64,
    pub total_messages: i64,
    pub created_at: String,
    pub updated_at: String,
}
