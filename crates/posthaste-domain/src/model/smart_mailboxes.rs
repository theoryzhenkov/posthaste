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
    pub kind: SmartMailboxKind,
    pub default_key: Option<String>,
    pub role: Option<String>,
    pub parent_id: Option<SmartMailboxId>,
    pub unread_messages: i64,
    pub total_messages: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Canonical order of the built-in smart mailboxes by `default_key`. This is the
/// single source of truth for default arrangement (replacing per-item integer
/// positions); it is the fallback order for any default not pinned by the user's
/// explicit [`AppSettings::smart_mailbox_order`](crate::AppSettings).
///
/// @spec docs/L1-accounts#sidebar-ordering
pub const DEFAULT_SMART_MAILBOX_ORDER: &[&str] = &[
    "inbox", "archive", "drafts", "sent", "junk", "trash", "all-mail",
];

/// Fallback sort rank for a smart mailbox when it is not pinned by the user's
/// explicit order: built-ins first in [`DEFAULT_SMART_MAILBOX_ORDER`], then user
/// mailboxes. The returned tuple sorts built-ins ahead of user mailboxes, and is
/// combined with `(created_at, name)` by the caller to break ties stably.
pub fn smart_mailbox_fallback_rank(default_key: Option<&str>) -> usize {
    default_key
        .and_then(|key| DEFAULT_SMART_MAILBOX_ORDER.iter().position(|k| *k == key))
        .unwrap_or(DEFAULT_SMART_MAILBOX_ORDER.len())
}

/// Reorder `items` to honor a user's explicit `order` of ids. Items whose id is
/// listed come first, in the listed sequence; the rest keep their incoming order
/// (the caller pre-sorts them by the domain's fallback order). Ids in `order`
/// with no matching item are ignored. This is the single ordering primitive
/// behind sidebar/settings drag-to-reorder for both smart mailboxes and
/// accounts — order is an explicit list, never a per-item integer.
///
/// @spec docs/L1-accounts#sidebar-ordering
pub fn apply_explicit_order<T>(
    items: Vec<T>,
    order: &[&str],
    id_of: impl Fn(&T) -> &str,
) -> Vec<T> {
    let rank: std::collections::HashMap<&str, usize> =
        order.iter().enumerate().map(|(i, id)| (*id, i)).collect();
    // `partition` is stable, so `rest` preserves the caller's fallback order.
    let (mut pinned, rest): (Vec<T>, Vec<T>) = items
        .into_iter()
        .partition(|item| rank.contains_key(id_of(item)));
    pinned.sort_by_key(|item| rank[id_of(item)]);
    pinned.into_iter().chain(rest).collect()
}

#[cfg(test)]
mod ordering_tests {
    use super::*;

    fn ids(items: Vec<&str>) -> Vec<String> {
        items.into_iter().map(str::to_string).collect()
    }

    #[test]
    fn pinned_ids_lead_in_listed_order_rest_keep_input_order() {
        let items = ids(vec!["a", "b", "c", "d"]);
        let order = ["c", "a"];
        let result = apply_explicit_order(items, &order, |s| s.as_str());
        // c, a pinned (in list order); b, d follow in their original order.
        assert_eq!(result, ids(vec!["c", "a", "b", "d"]));
    }

    #[test]
    fn stale_order_ids_are_ignored_and_unpinned_items_still_appear() {
        let items = ids(vec!["a", "b"]);
        let order = ["ghost", "b", "gone"];
        let result = apply_explicit_order(items, &order, |s| s.as_str());
        // Only "b" is real (pinned first); "a" was never in the list — it follows.
        assert_eq!(result, ids(vec!["b", "a"]));
    }

    #[test]
    fn empty_order_preserves_input_order() {
        let items = ids(vec!["x", "y", "z"]);
        let result = apply_explicit_order(items, &[], |s| s.as_str());
        assert_eq!(result, ids(vec!["x", "y", "z"]));
    }

    #[test]
    fn default_fallback_rank_orders_builtins_then_user() {
        assert_eq!(smart_mailbox_fallback_rank(Some("inbox")), 0);
        assert_eq!(smart_mailbox_fallback_rank(Some("trash")), 5);
        assert_eq!(smart_mailbox_fallback_rank(Some("all-mail")), 6);
        // Unknown/user mailboxes (no default_key) sort after every built-in.
        assert_eq!(
            smart_mailbox_fallback_rank(None),
            DEFAULT_SMART_MAILBOX_ORDER.len()
        );
        assert_eq!(
            smart_mailbox_fallback_rank(Some("nope")),
            DEFAULT_SMART_MAILBOX_ORDER.len()
        );
    }
}
