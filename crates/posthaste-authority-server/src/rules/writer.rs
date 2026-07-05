//! The GUI-managed rule **write surface** (RFC-L2-scripting ruling 23,
//! prerequisite 3). Writes go to `<config_root>/rules.d/<id>.toml`, one file per
//! rule, NEVER splicing the hand-authored `rules.toml` (which the GUI treats as
//! read-only and which may contain exec).
//!
//! Each file is a serialized domain [`Rule`] — the `when` tree round-trips
//! through TOML verbatim (matching the GET representation), so a create/read/edit
//! loop is lossless. Writes are atomic (write-temp → fsync → rename) so a crash
//! mid-write never leaves a half-written rule file the loader would choke on.
//!
//! **Exec can never reach here**: the REST body type is
//! [`WritableRuleAction`](posthaste_domain_model::WritableRuleAction), which has
//! no exec variant, so a [`Rule`] built for this path is exec-free by
//! construction. The extra runtime guard below is defence in depth.

use std::io::Write;
use std::path::{Path, PathBuf};

use posthaste_domain_model::{Rule, RuleAction};

use super::config::{validate_rule_grants, MANAGED_RULES_DIR};

/// A failure writing or deleting a GUI-managed rule file.
#[derive(Debug)]
pub enum RuleWriteError {
    /// The rule id contained path separators / traversal / null bytes.
    UnsafeId(String),
    /// The rule failed validation (empty-grant hook, or — impossibly, given the
    /// write body type — an exec action).
    Invalid(String),
    /// A create targeted an id that already exists (in `rules.d` or, on a
    /// caller check, `rules.toml`).
    Conflict(String),
    /// An update/delete targeted a managed rule id that does not exist.
    NotFound(String),
    /// The file could not be serialized or written.
    Io(String),
}

impl std::fmt::Display for RuleWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleWriteError::UnsafeId(id) => write!(f, "rule id '{id}' contains unsafe characters"),
            RuleWriteError::Invalid(message) => write!(f, "{message}"),
            RuleWriteError::Conflict(id) => write!(f, "a rule with id '{id}' already exists"),
            RuleWriteError::NotFound(id) => write!(f, "no managed rule with id '{id}'"),
            RuleWriteError::Io(message) => write!(f, "writing managed rule: {message}"),
        }
    }
}

impl std::error::Error for RuleWriteError {}

/// `<config_root>/rules.d`.
pub fn managed_rules_dir(config_root: &Path) -> PathBuf {
    config_root.join(MANAGED_RULES_DIR)
}

fn managed_rule_path(config_root: &Path, id: &str) -> Result<PathBuf, RuleWriteError> {
    validate_safe_id(id)?;
    Ok(managed_rules_dir(config_root).join(format!("{id}.toml")))
}

/// Whether a managed rule file exists for `id`.
pub fn managed_rule_exists(config_root: &Path, id: &str) -> bool {
    managed_rule_path(config_root, id)
        .map(|path| path.exists())
        .unwrap_or(false)
}

/// Atomically write a managed rule to `rules.d/<id>.toml`, overwriting any
/// existing file for that id (an update). The caller enforces create-vs-update
/// (conflict / not-found) semantics; this is the raw persist.
pub fn write_managed_rule(config_root: &Path, rule: &Rule) -> Result<(), RuleWriteError> {
    let path = managed_rule_path(config_root, &rule.id)?;
    // Defence in depth beside the structural WritableRuleAction guarantee.
    if matches!(rule.action, RuleAction::Exec { .. }) {
        return Err(RuleWriteError::Invalid(
            "exec actions are config-file-only and cannot be written via the managed store".into(),
        ));
    }
    validate_rule_grants(&rule.action)
        .map_err(|message| RuleWriteError::Invalid(message.to_string()))?;

    let body = toml::to_string_pretty(rule)
        .map_err(|error| RuleWriteError::Io(format!("serializing rule: {error}")))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| RuleWriteError::Io(error.to_string()))?;
    }
    atomic_write(&path, body.as_bytes()).map_err(|error| RuleWriteError::Io(error.to_string()))
}

/// Delete `rules.d/<id>.toml`. Errors [`RuleWriteError::NotFound`] if absent.
pub fn delete_managed_rule(config_root: &Path, id: &str) -> Result<(), RuleWriteError> {
    let path = managed_rule_path(config_root, id)?;
    if !path.exists() {
        return Err(RuleWriteError::NotFound(id.to_string()));
    }
    std::fs::remove_file(&path).map_err(|error| RuleWriteError::Io(error.to_string()))
}

/// Reject ids containing path separators, parent traversal, or null bytes — a
/// managed id becomes a filename, so this prevents path injection (mirrors the
/// config crate's `validate_safe_id`).
fn validate_safe_id(id: &str) -> Result<(), RuleWriteError> {
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains('\0')
    {
        return Err(RuleWriteError::UnsafeId(id.to_string()));
    }
    Ok(())
}

/// Write `content` to `path` atomically (write-temp → fsync → rename). A
/// per-call unique temp name means concurrent writes to the same id do not race
/// on a shared temp file (last-writer-wins, no torn file). Mirrors
/// `posthaste-config`'s `atomic_write` (not re-exported, so re-implemented here).
fn atomic_write(path: &Path, content: &[u8]) -> Result<(), std::io::Error> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent directory")
    })?;
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("rule");
    let temp_path = parent.join(format!("{file_name}.tmp.{}.{}", std::process::id(), unique));

    let result = (|| {
        let mut file = std::fs::File::create(&temp_path)?;
        file.write_all(content)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temp_path, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp_path);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use posthaste_domain_model::{
        RuleGrant, SmartMailboxGroup, SmartMailboxGroupOperator, SmartMailboxRule,
    };

    fn rule(id: &str, action: RuleAction) -> Rule {
        Rule {
            id: id.into(),
            name: "n".into(),
            when: SmartMailboxRule {
                root: SmartMailboxGroup {
                    operator: SmartMailboxGroupOperator::All,
                    negated: false,
                    nodes: Vec::new(),
                },
            },
            on: Vec::new(),
            action,
            enabled: true,
        }
    }

    #[test]
    fn write_then_read_round_trips_a_managed_rule() {
        let dir = tempfile::tempdir().expect("tempdir");
        let r = rule("r1", RuleAction::Tag { tag: "x".into() });
        write_managed_rule(dir.path(), &r).expect("write");
        assert!(managed_rule_exists(dir.path(), "r1"));

        let loaded = super::super::load_rules(dir.path()).expect("load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "r1");
        assert_eq!(loaded[0].action, RuleAction::Tag { tag: "x".into() });
    }

    #[test]
    fn write_rejects_an_unsafe_id() {
        let dir = tempfile::tempdir().expect("tempdir");
        let r = rule("../escape", RuleAction::Tag { tag: "x".into() });
        assert!(matches!(
            write_managed_rule(dir.path(), &r),
            Err(RuleWriteError::UnsafeId(_))
        ));
    }

    #[test]
    fn write_rejects_an_empty_grant_webhook() {
        let dir = tempfile::tempdir().expect("tempdir");
        let r = rule(
            "r1",
            RuleAction::Webhook {
                url: "http://127.0.0.1/h".into(),
                grants: Vec::new(),
                expiry_seconds: 3600,
            },
        );
        assert!(matches!(
            write_managed_rule(dir.path(), &r),
            Err(RuleWriteError::Invalid(_))
        ));
    }

    #[test]
    fn write_accepts_a_granted_webhook() {
        let dir = tempfile::tempdir().expect("tempdir");
        let r = rule(
            "r1",
            RuleAction::Webhook {
                url: "http://127.0.0.1/h".into(),
                grants: vec![RuleGrant::Read],
                expiry_seconds: 3600,
            },
        );
        write_managed_rule(dir.path(), &r).expect("write");
    }

    #[test]
    fn delete_missing_managed_rule_is_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(matches!(
            delete_managed_rule(dir.path(), "nope"),
            Err(RuleWriteError::NotFound(_))
        ));
    }

    #[test]
    fn delete_removes_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let r = rule("r1", RuleAction::Tag { tag: "x".into() });
        write_managed_rule(dir.path(), &r).expect("write");
        delete_managed_rule(dir.path(), "r1").expect("delete");
        assert!(!managed_rule_exists(dir.path(), "r1"));
    }
}
