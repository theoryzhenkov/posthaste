//! Loading rules from the config root's `rules.toml` (RFC-L2-scripting ruling 6:
//! rules are file-authored for beta; the settings UI is post-beta).
//!
//! `posthaste-config` has no filesystem watcher (reload is explicit/pull-based),
//! so rules follow the same discipline: load at engine startup, and re-load on a
//! `reload_config` refresh via [`load_rules`]. The `when` clause is a query
//! string parsed through the shared [`posthaste_query_grammar`] — one grammar
//! for smart mailboxes and rule triggers (ruling 4).

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

/// Load the rules from `<config_root>/rules.toml`. An absent file yields no
/// rules (rules are opt-in). Each rule's `when` string is compiled to a
/// [`SmartMailboxRule`](posthaste_domain_model::SmartMailboxRule) via the shared
/// query grammar.
pub fn load_rules(config_root: &Path) -> Result<Vec<Rule>, RuleConfigError> {
    let path = config_root.join("rules.toml");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path).map_err(RuleConfigError::Io)?;
    let doc: RulesToml = toml::from_str(&text).map_err(RuleConfigError::Toml)?;
    doc.rule.into_iter().map(RuleToml::into_rule).collect()
}

impl RuleToml {
    fn into_rule(self) -> Result<Rule, RuleConfigError> {
        let when = posthaste_query_grammar::parse_query(&self.when).map_err(|message| {
            RuleConfigError::Query {
                rule_id: self.id.clone(),
                message,
            }
        })?;
        // F1 (security review): a webhook/exec action with empty grants would
        // mint an action-unrestricted token (see rule_minter). Reject at load
        // so the misconfig never reaches the bus — defence-in-depth beside the
        // minter guard.
        let empty_grants = match &self.action {
            RuleAction::Webhook { grants, .. } | RuleAction::Exec { grants, .. } => {
                grants.is_empty()
            }
            _ => false,
        };
        if empty_grants {
            return Err(RuleConfigError::Query {
                rule_id: self.id.clone(),
                message: "a webhook/exec rule must declare at least one grant"
                    .to_string(),
            });
        }
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
