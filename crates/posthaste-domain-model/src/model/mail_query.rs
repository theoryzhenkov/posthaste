//! The mail-query AST: one query system, several front-ends.
//!
//! This is the single, front-end-agnostic query language shared by every surface
//! that filters mail — smart mailboxes (saved queries with display metadata),
//! automation-rule WHEN-clauses, and any future consumer. The types here describe
//! *what to match* (fields, operators, values, boolean groups); they carry no
//! presentation or storage concern of their own. The `SmartMailbox` container and
//! its wire/ordering machinery live alongside in `smart_mailboxes.rs`; the store
//! SQL compiler and the canonical field schema (`query_schema.rs`) both build on
//! these types.
//!
//! @spec docs/L1-accounts#condition-fields-and-operators

use super::*;

/// Boolean combinator for smart mailbox rule groups: `All` (AND) or `Any` (OR).
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum MailQueryGroupOperator {
    All,
    Any,
}

/// Message field that a smart mailbox condition can filter on.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum MailQueryField {
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
    /// `to_json` recipient list. Cc/Bcc DO have their own columns now, but the
    /// query grammar has no term for them, so only the `To` recipient set is
    /// queryable (see field compiler). Adding one is a grammar term plus a
    /// compiler arm — no longer a schema change.
    To,
    Subject,
    Preview,
    /// Full message body text, matched via the FTS5 `message_fts` index (the
    /// `body` column, fed from the body cache's `message_body.body_text`).
    /// `contains` is a *token/phrase* match (porter-stemmed, diacritics
    /// removed, last token treated as a prefix) — not a raw substring `LIKE`
    /// like [`Preview`](Self::Preview). A message whose body has not been
    /// cached yet is not body-searchable until the cache warms it.
    Body,
    ReceivedAt,
    /// Message byte size (`message.size`), compared numerically. Reuses the
    /// neutral ordered operators (`Lt`/`Gt`/`Le`/`Ge`) as `< > <= >=`; the
    /// emitted value is a byte count encoded as a string.
    Size,
}

/// Comparison operator for a smart mailbox condition.
///
/// The four ordered comparisons are **neutral** (`Lt`/`Gt`/`Le`/`Ge`, i.e.
/// `< > <= >=`): the model no longer speaks "date" — dates and numbers share the
/// same comparators, and the editor labels them per field type ("before/after"
/// for dates, "smaller/larger than" for size). See D6 of RFC-L2-query-schema.
///
/// BACK-COMPAT (critical — these are stored wire names): the four ordered
/// variants carry a `#[serde(alias = ...)]` for their OLD camelCase names
/// (`before`/`after`/`onOrBefore`/`onOrAfter`), so smart mailboxes / rules
/// persisted before the rename still deserialize. Serialization emits the NEW
/// names (`lt`/`gt`/`le`/`ge`); no migration is needed — old data reads, re-saved
/// data uses the new names.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum MailQueryOperator {
    Equals,
    In,
    Contains,
    /// Case-insensitive prefix match (`beginsWith`): the text starts with the
    /// value. Compiles to a `LIKE '<value>%'` with the value's LIKE metacharacters
    /// escaped, so a literal `%`/`_` matches itself.
    BeginsWith,
    /// Case-insensitive suffix match (`endsWith`): the text ends with the value.
    /// Compiles to a `LIKE '%<value>'` (metacharacters escaped).
    EndsWith,
    /// Regular-expression match (`regex`): the value is a regex pattern compiled
    /// by the `regex` crate and evaluated via the store's registered `regexp`
    /// SQLite scalar. A malformed pattern is rejected at the write boundary
    /// (`validate_condition`), so it never reaches the store.
    Regex,
    /// `<` — legacy wire name `before`.
    #[serde(alias = "before")]
    Lt,
    /// `>` — legacy wire name `after`.
    #[serde(alias = "after")]
    Gt,
    /// `<=` — legacy wire name `onOrBefore`.
    #[serde(alias = "onOrBefore")]
    Le,
    /// `>=` — legacy wire name `onOrAfter`.
    #[serde(alias = "onOrAfter")]
    Ge,
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
pub enum MailQueryValue {
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
/// untagged [`MailQueryValue`] tell a date apart from a bare string.
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
pub struct MailQueryGroup {
    pub operator: MailQueryGroupOperator,
    pub negated: bool,
    // Break the MailQueryGroup -> MailQueryRuleNode -> MailQueryGroup
    // schema cycle so utoipa's component collector does not recurse infinitely.
    // The emitted schema still references MailQueryRuleNode by `$ref`.
    #[cfg_attr(feature = "openapi", schema(no_recursion))]
    pub nodes: Vec<MailQueryRuleNode>,
}

/// Leaf condition matching a single field with an operator and value.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MailQueryCondition {
    pub field: MailQueryField,
    pub operator: MailQueryOperator,
    pub negated: bool,
    pub value: MailQueryValue,
}

/// Recursive rule tree node: either a [`MailQueryGroup`] or a [`MailQueryCondition`].
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum MailQueryRuleNode {
    Group(MailQueryGroup),
    Condition(MailQueryCondition),
}

/// Top-level rule for a smart mailbox, wrapping a root group.
///
/// @spec docs/L1-accounts#condition-fields-and-operators
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MailQueryRule {
    pub root: MailQueryGroup,
}

#[cfg(test)]
mod value_serde_tests {
    use super::*;

    #[test]
    fn legacy_scalar_shapes_still_deserialize() {
        // Back-compat: the pre-existing untagged shapes are unchanged.
        assert_eq!(
            serde_json::from_str::<MailQueryValue>(r#""2026-01-01T00:00:00Z""#).unwrap(),
            MailQueryValue::String("2026-01-01T00:00:00Z".to_string())
        );
        assert_eq!(
            serde_json::from_str::<MailQueryValue>(r#"["a","b"]"#).unwrap(),
            MailQueryValue::Strings(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(
            serde_json::from_str::<MailQueryValue>("true").unwrap(),
            MailQueryValue::Bool(true)
        );
    }

    #[test]
    fn date_relative_round_trips_tagged() {
        let value = MailQueryValue::Date(DateValue::Relative {
            amount: 7,
            unit: DateUnit::Days,
        });
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "relative", "amount": 7, "unit": "days" })
        );
        assert_eq!(
            serde_json::from_value::<MailQueryValue>(json).unwrap(),
            value
        );
    }

    #[test]
    fn date_absolute_round_trips_tagged() {
        let value = MailQueryValue::Date(DateValue::Absolute {
            value: "2026-07-06T00:00:00Z".to_string(),
        });
        let json = serde_json::to_value(&value).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "kind": "absolute", "value": "2026-07-06T00:00:00Z" })
        );
        assert_eq!(
            serde_json::from_value::<MailQueryValue>(json).unwrap(),
            value
        );
    }

    #[test]
    fn legacy_operator_names_deserialize_to_neutral_variants() {
        // BACK-COMPAT: stored rules use the OLD wire names; they must still read.
        for (legacy, expected) in [
            ("before", MailQueryOperator::Lt),
            ("after", MailQueryOperator::Gt),
            ("onOrBefore", MailQueryOperator::Le),
            ("onOrAfter", MailQueryOperator::Ge),
        ] {
            let json = format!("\"{legacy}\"");
            assert_eq!(
                serde_json::from_str::<MailQueryOperator>(&json).unwrap(),
                expected,
                "legacy operator {legacy:?} must deserialize to the neutral variant"
            );
        }
        // The unchanged operators still round-trip by their own names.
        assert_eq!(
            serde_json::from_str::<MailQueryOperator>("\"equals\"").unwrap(),
            MailQueryOperator::Equals
        );
    }

    #[test]
    fn neutral_operators_serialize_with_new_names() {
        // Re-saved data uses the NEW neutral wire names.
        for (operator, wire) in [
            (MailQueryOperator::Lt, "lt"),
            (MailQueryOperator::Gt, "gt"),
            (MailQueryOperator::Le, "le"),
            (MailQueryOperator::Ge, "ge"),
            (MailQueryOperator::Equals, "equals"),
            (MailQueryOperator::In, "in"),
            (MailQueryOperator::Contains, "contains"),
        ] {
            assert_eq!(
                serde_json::to_value(operator).unwrap(),
                serde_json::json!(wire)
            );
        }
    }

    #[test]
    fn text_match_operators_use_camel_case() {
        // The additive text operators (R4) round-trip by their camelCase wire names.
        for (operator, wire) in [
            (MailQueryOperator::BeginsWith, "beginsWith"),
            (MailQueryOperator::EndsWith, "endsWith"),
            (MailQueryOperator::Regex, "regex"),
        ] {
            assert_eq!(
                serde_json::to_value(operator).unwrap(),
                serde_json::json!(wire)
            );
            assert_eq!(
                serde_json::from_str::<MailQueryOperator>(&format!("\"{wire}\"")).unwrap(),
                operator
            );
        }
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
