//! The install manifest: a durable record of every component the wizard has
//! installed, so `posthaste-wizard update` (RFC-L2-scripting ruling 14, the
//! headless/self-host updater) knows *what* is installed, *where*, at *which*
//! version, and on *which* channel — the four facts an updater needs and the
//! only actor (the wizard owns the service units) that can act on them.
//!
//! Shape: a small TOML at `$XDG_STATE_HOME/posthaste/wizard-manifest.toml`
//! (else `~/.local/state/posthaste/wizard-manifest.toml`) — the XDG *state*
//! dir, distinct from the daemon's *data* dir (`daemon.json`) and the wizard's
//! own service-unit *config* dir, matching each XDG category's meaning. A
//! `POSTHASTE_WIZARD_MANIFEST` override exists for tests and unusual layouts.
//!
//! Retrofit tolerance (ruling 14): the manifest did not exist before this
//! landed, so a missing file reads back as an empty manifest — `update` then
//! degrades to a re-install/`--from` detection rather than erroring. Every
//! install path (role `install`, `ctl install`) records into it from now on.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One recorded component. Keyed by [`component`](Self::component) (the binary
/// name), which is unique across a host's install set.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Component {
    /// The installed binary's file name, e.g. `posthaste-authority-server`,
    /// `posthastectl`, `posthaste-wizard`. The manifest's unique key.
    pub component: String,
    /// What kind of component this is: `role` | `ctl` | `wizard`. Selects the
    /// release artifact naming and whether a service unit is involved.
    pub kind: String,
    /// The absolute path the binary was installed to.
    pub path: String,
    /// The concrete version installed (e.g. `0.2.0-nightly.50`), resolved from
    /// the channel's updater manifest at install time — not the rolling tag.
    pub version: String,
    /// The release channel: `nightly` | `stable`.
    pub channel: String,
    /// When it was installed/last updated (RFC3339 UTC).
    pub installed_at: String,
    /// For a `role` component: the service manager that owns its unit, so
    /// `update` can stop/start it around the swap. One of `user-systemd` |
    /// `system-systemd` | `launchd`. `None` for `ctl`/`wizard` (no unit) or a
    /// `--no-service` install.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub service: Option<String>,
    /// For a `role` component with a service: the unit name (systemd) or plist
    /// path (launchd) `update` hands to the service manager.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub unit: Option<String>,
    /// The version this component held before the most recent `update` swap, so
    /// `--rollback` can restore both the `.bak` binary *and* the recorded
    /// version. Set by `update`, cleared on rollback.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub previous_version: Option<String>,
}

/// The whole manifest: a schema tag plus the component list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default = "one")]
    pub schema_version: u32,
    #[serde(default, rename = "component")]
    pub components: Vec<Component>,
}

fn one() -> u32 {
    1
}

impl Default for Manifest {
    fn default() -> Self {
        // A fresh (or retrofit-missing) manifest is schema v1 with no
        // components — not the derived all-zero, which would write
        // `schema_version = 0`.
        Manifest {
            schema_version: 1,
            components: Vec::new(),
        }
    }
}

impl Manifest {
    /// Load the manifest at `path`. A **missing file is not an error** — it
    /// reads back as an empty manifest (retrofit tolerance, ruling 14). Only a
    /// present-but-corrupt file is an error the caller should surface.
    pub fn load(path: &Path) -> Result<Manifest, String> {
        match std::fs::read_to_string(path) {
            Ok(raw) => toml::from_str(&raw)
                .map_err(|e| format!("parse wizard manifest {}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Manifest::default()),
            Err(e) => Err(format!("read wizard manifest {}: {e}", path.display())),
        }
    }

    /// Insert or replace the entry for `entry.component` (upsert by binary
    /// name), so re-installing or updating a component never duplicates it.
    pub fn record(&mut self, entry: Component) {
        if let Some(slot) = self
            .components
            .iter_mut()
            .find(|c| c.component == entry.component)
        {
            *slot = entry;
        } else {
            self.components.push(entry);
        }
    }

    /// Look up a component by binary name.
    pub fn get(&self, component: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.component == component)
    }

    /// Write the manifest to `path`, creating the parent directory.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create manifest dir {}: {e}", parent.display()))?;
        }
        let body =
            toml::to_string_pretty(self).map_err(|e| format!("serialize wizard manifest: {e}"))?;
        std::fs::write(path, body).map_err(|e| format!("write {}: {e}", path.display()))
    }
}

/// The default manifest path: `$POSTHASTE_WIZARD_MANIFEST`, else
/// `$XDG_STATE_HOME/posthaste/wizard-manifest.toml`, else
/// `~/.local/state/posthaste/wizard-manifest.toml`.
pub fn manifest_path() -> PathBuf {
    if let Some(explicit) = std::env::var_os("POSTHASTE_WIZARD_MANIFEST") {
        return PathBuf::from(explicit);
    }
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from(".local/state"));
    base.join("posthaste").join("wizard-manifest.toml")
}

/// An RFC3339 UTC timestamp for `installed_at`.
pub fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, version: &str) -> Component {
        Component {
            component: name.to_string(),
            kind: "role".to_string(),
            path: format!("/home/u/.local/bin/{name}"),
            version: version.to_string(),
            channel: "nightly".to_string(),
            installed_at: "2026-07-04T00:00:00Z".to_string(),
            service: Some("user-systemd".to_string()),
            unit: Some(format!("{name}.service")),
            previous_version: None,
        }
    }

    #[test]
    fn round_trips_through_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wizard-manifest.toml");
        let mut m = Manifest::default();
        m.record(entry("posthaste-authority-server", "0.2.0-nightly.44"));
        m.record(Component {
            service: None,
            unit: None,
            kind: "ctl".to_string(),
            ..entry("posthastectl", "0.2.0-nightly.44")
        });
        m.save(&path).unwrap();

        let back = Manifest::load(&path).unwrap();
        assert_eq!(back.components.len(), 2);
        assert_eq!(
            back.get("posthaste-authority-server").unwrap().version,
            "0.2.0-nightly.44"
        );
        let ctl = back.get("posthastectl").unwrap();
        assert_eq!(ctl.kind, "ctl");
        assert!(ctl.service.is_none(), "ctl has no service");
    }

    #[test]
    fn missing_file_reads_as_empty_manifest() {
        // Retrofit tolerance: `update` on a host that installed before the
        // manifest existed must see an empty manifest, not an error.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does-not-exist.toml");
        let m = Manifest::load(&path).expect("missing file is not an error");
        assert!(m.components.is_empty());
        assert_eq!(m.schema_version, 1);
    }

    #[test]
    fn record_upserts_by_component_name() {
        let mut m = Manifest::default();
        m.record(entry("posthaste-runtime", "0.2.0-nightly.44"));
        m.record(entry("posthaste-runtime", "0.2.0-nightly.50"));
        assert_eq!(
            m.components.len(),
            1,
            "same component upserts, not duplicates"
        );
        assert_eq!(
            m.get("posthaste-runtime").unwrap().version,
            "0.2.0-nightly.50"
        );
    }

    #[test]
    fn corrupt_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wizard-manifest.toml");
        std::fs::write(&path, "this is not = valid = toml =").unwrap();
        assert!(Manifest::load(&path).is_err());
    }
}
