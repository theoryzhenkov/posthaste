//! Loading rules from the config root. There are **two** rule stores, merged at
//! load (RFC-L2-scripting ruling 23, survey prerequisite 3):
//!
//! * `rules.toml` — **hand-authored**, read-only to the GUI. Its `when` is a
//!   query string parsed through the shared [`posthaste_query_grammar`] (ruling
//!   4: one grammar). It MAY contain [`RuleAction::Exec`] — the config file is
//!   the only place exec is ever settable (threat 3).
//! * `rules.d/*.toml` — **GUI-managed** (see [`super::writer`]), one file per
//!   rule id, exec-free by construction. Each file is a serialized domain
//!   [`Rule`] (the `when` tree round-trips through TOML verbatim, so no grammar
//!   pass is needed — the GUI already sends a parsed tree). The loader
//!   defensively **skips** any managed file carrying an exec action, so a
//!   hand-dropped exec in `rules.d` never runs (exec belongs in `rules.toml`).
//!
//! **Precedence:** on an id collision between the two stores, the hand-authored
//! `rules.toml` rule WINS and the colliding `rules.d` file is ignored (logged).
//! Rationale: `rules.toml` is the higher-trust, exec-capable source; the GUI
//! must not be able to shadow a hand-authored rule. GUI-created ids are UUIDs,
//! so a collision is a deliberate act, not an accident.
//!
//! `posthaste-config` has no filesystem watcher (reload is explicit/pull-based),
//! so rules follow the same discipline: load at engine startup, and re-load on a
//! write via [`load_rules`] (the reload path, prerequisite 2).

use std::collections::HashSet;
use std::path::Path;

use posthaste_domain_model::{Rule, RuleAction};
use serde::Deserialize;

/// The `rules.toml` document: a list of `[[rule]]` tables.
#[derive(Debug, Default, Deserialize)]
struct RulesToml {
    #[serde(default)]
    rule: Vec<RuleToml>,
}

/// One `[[rule]]` table. `when` is a query string (parsed via the shared
/// grammar); `action` reuses the domain [`RuleAction`] enum verbatim (an inline
/// or sub-table `{ kind = "…", … }`, camelCase fields).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleToml {
    id: String,
    name: String,
    when: String,
    #[serde(default)]
    on: Vec<String>,
    #[serde(default = "default_enabled")]
    enabled: bool,
    action: RuleAction,
}

fn default_enabled() -> bool {
    true
}

/// A failure loading or parsing `rules.toml`.
#[derive(Debug)]
pub enum RuleConfigError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The TOML document was malformed.
    Toml(toml::de::Error),
    /// A rule's `when` query did not parse under the shared grammar.
    Query { rule_id: String, message: String },
}

impl std::fmt::Display for RuleConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleConfigError::Io(error) => write!(f, "reading rules.toml: {error}"),
            RuleConfigError::Toml(error) => write!(f, "parsing rules.toml: {error}"),
            RuleConfigError::Query { rule_id, message } => {
                write!(f, "rule {rule_id}: invalid `when` query: {message}")
            }
        }
    }
}

impl std::error::Error for RuleConfigError {}

/// The GUI-managed rules subdirectory under the config root (prerequisite 3).
/// Loaded and written per-file, one `<id>.toml` per rule; exec-free by
/// construction. See [`super::writer`] for the write side.
pub const MANAGED_RULES_DIR: &str = "rules.d";

/// Load the merged ruleset: the hand-authored `rules.toml` PLUS the GUI-managed
/// `rules.d/*.toml`, with `rules.toml` winning on any id collision (see the
/// module docs). An absent `rules.toml` and an absent `rules.d/` both yield
/// nothing (rules are opt-in).
///
/// `rules.toml` parse errors are hard failures (it is the operator's own,
/// authored file). A single malformed or exec-carrying `rules.d` file is
/// **skipped with a warning** rather than failing the whole load, so one bad
/// GUI file cannot take down every rule (including the authored ones).
pub fn load_rules(config_root: &Path) -> Result<Vec<Rule>, RuleConfigError> {
    let mut rules = load_authored_rules(config_root)?;
    let authored_ids: HashSet<String> = rules.iter().map(|rule| rule.id.clone()).collect();

    for managed in load_managed_rules(config_root) {
        if authored_ids.contains(&managed.id) {
            tracing::warn!(
                rule_id = %managed.id,
                "rules.d/{}.toml is shadowed by a rules.toml rule of the same id; ignoring the managed copy",
                managed.id
            );
            continue;
        }
        rules.push(managed);
    }
    Ok(rules)
}

/// Load ONLY the hand-authored `rules.toml` (query-string `when`). An absent
/// file yields no rules.
fn load_authored_rules(config_root: &Path) -> Result<Vec<Rule>, RuleConfigError> {
    let path = config_root.join("rules.toml");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path).map_err(RuleConfigError::Io)?;
    let doc: RulesToml = toml::from_str(&text).map_err(RuleConfigError::Toml)?;
    doc.rule.into_iter().map(RuleToml::into_rule).collect()
}

/// Load the GUI-managed `rules.d/*.toml` files (tree `when`). Best-effort: a
/// file that fails to read/parse, fails validation, or carries an exec action is
/// skipped with a warning (see [`load_rules`]). Returned unsorted; the caller
/// merges. An absent directory yields nothing.
fn load_managed_rules(config_root: &Path) -> Vec<Rule> {
    let dir = config_root.join(MANAGED_RULES_DIR);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(), // absent or unreadable ⇒ no managed rules
    };
    let mut rules = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|ext| ext != "toml").unwrap_or(true) {
            continue;
        }
        match parse_managed_rule(&path) {
            Ok(rule) => rules.push(rule),
            Err(reason) => {
                tracing::warn!(path = %path.display(), reason, "skipping invalid rules.d file");
            }
        }
    }
    rules
}

/// Parse and validate one `rules.d/<id>.toml` file into a [`Rule`]. The `when`
/// tree is already parsed (round-tripped through TOML), so no grammar pass is
/// needed. Rejects exec actions (managed store is exec-free) and empty-grant
/// hooks (the F1 minter guard).
fn parse_managed_rule(path: &Path) -> Result<Rule, String> {
    let text = std::fs::read_to_string(path).map_err(|error| format!("reading: {error}"))?;
    let rule: Rule = toml::from_str(&text).map_err(|error| format!("parsing: {error}"))?;
    // Defence in depth: the write path (WritableRuleAction) cannot produce exec,
    // but a hand-dropped exec file must never run from the managed store — exec
    // is `rules.toml`-only (threat 3).
    if matches!(rule.action, RuleAction::Exec { .. }) {
        return Err("exec actions are not permitted in rules.d (use rules.toml)".to_string());
    }
    validate_rule_grants(&rule.action).map_err(|message| message.to_string())?;
    Ok(rule)
}

/// The F1 (security review) rule: a webhook/exec action with empty grants would
/// mint an action-unrestricted token. Shared by the authored loader, the managed
/// loader, and the write path so no store can smuggle one onto the bus.
pub(crate) fn validate_rule_grants(action: &RuleAction) -> Result<(), &'static str> {
    let empty_grants = match action {
        RuleAction::Webhook { grants, .. } | RuleAction::Exec { grants, .. } => grants.is_empty(),
        _ => false,
    };
    if empty_grants {
        return Err("a webhook/exec rule must declare at least one grant");
    }
    Ok(())
}

impl RuleToml {
    fn into_rule(self) -> Result<Rule, RuleConfigError> {
        let when = posthaste_query_grammar::parse_query(&self.when).map_err(|message| {
            RuleConfigError::Query {
                rule_id: self.id.clone(),
                message,
            }
        })?;
        // F1 (security review): reject an empty-grant hook at load so the
        // misconfig never reaches the bus — defence-in-depth beside the minter.
        validate_rule_grants(&self.action).map_err(|message| RuleConfigError::Query {
            rule_id: self.id.clone(),
            message: message.to_string(),
        })?;
        Ok(Rule {
            id: self.id,
            name: self.name,
            when,
            on: self.on,
            action: self.action,
            enabled: self.enabled,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain_model::RuleGrant;

    fn write_rules(dir: &Path, body: &str) {
        std::fs::write(dir.join("rules.toml"), body).expect("write rules.toml");
    }

    #[test]
    fn a_webhook_rule_with_empty_grants_is_rejected() {
        // F1 security-review regression: empty grants would mint an
        // action-unrestricted token; the loader must reject it.
        let dir = tempfile::tempdir().expect("tempdir");
        write_rules(
            dir.path(),
            r#"
[[rule]]
id = "r"
name = "n"
when = "tag:x"
action = { kind = "webhook", url = "http://127.0.0.1:9/h", grants = [] }
"#,
        );
        let err = load_rules(dir.path()).expect_err("empty grants must be rejected");
        assert!(matches!(err, RuleConfigError::Query { .. }));
    }

    #[test]
    fn absent_file_yields_no_rules() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(load_rules(dir.path()).expect("load").is_empty());
    }

    #[test]
    fn parses_webhook_rule_with_query_when() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_rules(
            dir.path(),
            r#"
[[rule]]
id = "instruct-agent"
name = "Send instruct-tagged mail to the agent"
when = "tag:instruct"
enabled = true
action = { kind = "webhook", url = "http://127.0.0.1:9/hook", grants = ["read", "tag"], expirySeconds = 3600 }
"#,
        );
        let rules = load_rules(dir.path()).expect("load");
        assert_eq!(rules.len(), 1);
        let rule = &rules[0];
        assert_eq!(rule.id, "instruct-agent");
        assert!(rule.enabled);
        match &rule.action {
            RuleAction::Webhook {
                url,
                grants,
                expiry_seconds,
            } => {
                assert_eq!(url, "http://127.0.0.1:9/hook");
                assert_eq!(grants, &vec![RuleGrant::Read, RuleGrant::Tag]);
                assert_eq!(*expiry_seconds, 3600);
            }
            other => panic!("expected webhook action, got {other:?}"),
        }
    }

    fn write_managed(dir: &Path, id: &str, body: &str) {
        let managed = dir.join(MANAGED_RULES_DIR);
        std::fs::create_dir_all(&managed).expect("mkdir rules.d");
        std::fs::write(managed.join(format!("{id}.toml")), body).expect("write managed rule");
    }

    /// A serialized domain `Rule` (tree `when`) in `rules.d/*.toml` loads and
    /// merges with the authored `rules.toml`.
    #[test]
    fn merges_authored_and_managed_rules() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_rules(
            dir.path(),
            r#"
[[rule]]
id = "authored"
name = "authored"
when = "tag:a"
action = { kind = "tag", tag = "a" }
"#,
        );
        write_managed(
            dir.path(),
            "managed",
            r#"
id = "managed"
name = "managed"
enabled = true
[when.root]
operator = "all"
negated = false
[[when.root.nodes]]
type = "condition"
field = "keyword"
operator = "equals"
negated = false
value = "b"
[action]
kind = "tag"
tag = "b"
"#,
        );
        let mut ids: Vec<_> = load_rules(dir.path())
            .expect("load")
            .into_iter()
            .map(|rule| rule.id)
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["authored".to_string(), "managed".to_string()]);
    }

    /// On an id collision, the hand-authored `rules.toml` rule wins and the
    /// `rules.d` copy is ignored (precedence rule).
    #[test]
    fn authored_rule_shadows_managed_on_id_collision() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_rules(
            dir.path(),
            r#"
[[rule]]
id = "dup"
name = "authored-wins"
when = "tag:a"
action = { kind = "tag", tag = "authored" }
"#,
        );
        write_managed(
            dir.path(),
            "dup",
            r#"
id = "dup"
name = "managed-loses"
enabled = true
[when.root]
operator = "all"
negated = false
[action]
kind = "tag"
tag = "managed"
"#,
        );
        let rules = load_rules(dir.path()).expect("load");
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].name, "authored-wins");
        assert_eq!(
            rules[0].action,
            RuleAction::Tag {
                tag: "authored".into()
            }
        );
    }

    /// A hand-dropped exec action in `rules.d` is skipped (never runs from the
    /// managed store); the rest of the load succeeds.
    #[test]
    fn managed_exec_rule_is_skipped_not_loaded() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_managed(
            dir.path(),
            "sneaky",
            r#"
id = "sneaky"
name = "sneaky"
enabled = true
[when.root]
operator = "all"
negated = false
[action]
kind = "exec"
command = "/bin/rm"
grants = ["read"]
"#,
        );
        assert!(
            load_rules(dir.path()).expect("load").is_empty(),
            "an exec rule in rules.d must be skipped"
        );
    }

    #[test]
    fn invalid_when_query_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        write_rules(
            dir.path(),
            r#"
[[rule]]
id = "bad"
name = "bad"
when = "is:notarealvalue"
action = { kind = "tag", tag = "x" }
"#,
        );
        assert!(matches!(
            load_rules(dir.path()),
            Err(RuleConfigError::Query { .. })
        ));
    }
}
