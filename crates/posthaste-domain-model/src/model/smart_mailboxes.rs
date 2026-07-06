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
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
    /// Recipient (`To`) address or display name, matched against the stored
    /// `to_json` recipient list. Cc/Bcc are not projected as separate columns
    /// today, so only the `To` recipient set is queryable (see field compiler).
    To,
    Subject,
    Preview,
    ReceivedAt,
    /// Message byte size (`message.size`), compared numerically. Reuses the
    /// inequality operators (`Before`/`After`/`OnOrBefore`/`OnOrAfter`) as
    /// `< > <= >=`; the emitted value is a byte count encoded as a string.
    Size,
}

/// Comparison operator for a smart mailbox condition.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
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

/// Condition value: scalar string, string list (for `In`), boolean, or a typed
/// date value.
///
/// The enum is `#[serde(untagged)]`: each variant is distinguished by its JSON
/// *shape*, so the legacy `String`/`Strings`/`Bool` values still deserialize
/// exactly as before (a bare string, a string array, a bare boolean). The
/// [`Date`](Self::Date) variant is a JSON *object* carrying a `kind`
/// discriminator, a shape none of the scalar variants accept, so adding it is
/// fully back-compatible and needs no migration of stored data — legacy
/// absolute dates persisted as a bare `String` keep parsing (see the date field
/// compiler, which reads both the legacy string and the new `Date::Absolute`).
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum SmartMailboxValue {
    String(String),
    Strings(Vec<String>),
    Bool(bool),
    /// A typed date value: either an absolute instant or a rolling relative
    /// offset. Distinguished from the scalar variants by being a JSON object
    /// with a `kind` tag.
    Date(DateValue),
}

/// A date condition value. Tagged (internally, on `kind`) so absolute and
/// relative dates are explicit, distinct JSON objects — this is what lets the
/// untagged [`SmartMailboxValue`] tell a date apart from a bare string.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum DateValue {
    /// An absolute RFC3339 instant, compared against `received_at` as stored
    /// (the same literal comparison legacy bare-string dates always used).
    Absolute { value: String },
    /// A rolling relative offset ("N units ago"), stored as-is and resolved to
    /// an instant at *query* time so the window rolls with `now` instead of
    /// freezing to a fixed date at edit time.
    Relative { amount: u32, unit: DateUnit },
}

/// Time unit for a [`DateValue::Relative`] offset.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum DateUnit {
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
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
mod value_serde_tests {
    use super::*;

    #[test]
    fn legacy_scalar_shapes_still_deserialize() {
        // Back-compat: the pre-existing untagged shapes are unchanged.
        assert_eq!(
            serde_json::from_str::<SmartMailboxValue>(r#""2026-01-01T00:00:00Z""#).unwrap(),
            SmartMailboxValue::String("2026-01-01T00:00:00Z".to_string())
        );
        assert_eq!(
            serde_json::from_str::<SmartMailboxValue>(r#"["a","b"]"#).unwrap(),
            SmartMailboxValue::Strings(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(
            serde_json::from_str::<SmartMailboxValue>("true").unwrap(),
            SmartMailboxValue::Bool(true)
        );
    }

    #[test]
    fn date_relative_round_trips_tagged() {
        let value = SmartMailboxValue::Date(DateValue::Relative {
            amount: 7,
            unit: DateUnit::Days,
        });
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "relative", "amount": 7, "unit": "days" })
        );
        assert_eq!(
            serde_json::from_value::<SmartMailboxValue>(json).unwrap(),
            value
        );
    }

    #[test]
    fn date_absolute_round_trips_tagged() {
        let value = SmartMailboxValue::Date(DateValue::Absolute {
            value: "2026-07-06T00:00:00Z".to_string(),
        });
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "absolute", "value": "2026-07-06T00:00:00Z" })
        );
        assert_eq!(
            serde_json::from_value::<SmartMailboxValue>(json).unwrap(),
            value
        );
    }

    #[test]
    fn all_date_units_use_camel_case() {
        for (unit, wire) in [
            (DateUnit::Minutes, "minutes"),
            (DateUnit::Hours, "hours"),
            (DateUnit::Days, "days"),
            (DateUnit::Weeks, "weeks"),
            (DateUnit::Months, "months"),
        ] {
            assert_eq!(serde_json::to_value(unit).unwrap(), serde_json::json!(wire));
        }
    }
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
