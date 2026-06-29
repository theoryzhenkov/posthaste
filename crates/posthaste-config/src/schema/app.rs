use super::*;

// -- app.toml --

/// TOML representation of the global `app.toml` config file.
///
/// @spec docs/L1-accounts#apptoml
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AppToml {
    #[serde(default)]
    pub schema_version: u32,
    pub default_source_id: Option<String>,
    #[serde(default)]
    pub automations: Vec<AutomationRuleToml>,
    #[serde(default)]
    pub draft_automations: Vec<AutomationRuleToml>,
    #[serde(default)]
    pub daemon: DaemonToml,
    #[serde(default)]
    pub logging: LoggingToml,
    #[serde(default)]
    pub cache: CachePolicyToml,
    #[serde(default)]
    pub appearance: Option<AppearanceToml>,
    #[serde(default)]
    pub link: LinkToml,
}

/// Runtime↔backend link settings (`[link]`). Default: in-process, link not
/// served — the bundled single-process deployment is unaffected.
///
/// @spec docs/replication/L1#10-deployment-topology
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LinkToml {
    /// Backend role: serve the runtime↔backend link over HTTP for a remote
    /// runtime. Default `false`.
    pub serve: Option<bool>,
    /// Shared bearer token — required from connecting runtimes (serve role) and
    /// presented to the remote backend (connect role).
    pub token: Option<String>,
    /// Runtime role: connect to a remote backend at this base URL instead of the
    /// in-process one. When set, this process is a near node over the link.
    pub backend_url: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LoggingToml {
    pub level: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct CachePolicyToml {
    pub soft_cap_bytes: Option<u64>,
    pub hard_cap_bytes: Option<u64>,
    pub cache_bodies: Option<bool>,
    pub cache_raw_messages: Option<bool>,
    pub cache_attachments: Option<bool>,
}

impl CachePolicyToml {
    fn to_cache_policy(&self) -> CachePolicy {
        let default = CachePolicy::default();
        CachePolicy {
            soft_cap_bytes: self.soft_cap_bytes.unwrap_or(default.soft_cap_bytes),
            hard_cap_bytes: self
                .hard_cap_bytes
                .unwrap_or(default.hard_cap_bytes)
                .max(self.soft_cap_bytes.unwrap_or(default.soft_cap_bytes)),
            cache_bodies: self.cache_bodies.unwrap_or(default.cache_bodies),
            cache_raw_messages: self
                .cache_raw_messages
                .unwrap_or(default.cache_raw_messages),
            cache_attachments: self.cache_attachments.unwrap_or(default.cache_attachments),
        }
    }

    fn from_cache_policy(policy: &CachePolicy) -> Self {
        Self {
            soft_cap_bytes: Some(policy.soft_cap_bytes),
            hard_cap_bytes: Some(policy.hard_cap_bytes),
            cache_bodies: Some(policy.cache_bodies),
            cache_raw_messages: Some(policy.cache_raw_messages),
            cache_attachments: Some(policy.cache_attachments),
        }
    }
}

/// TOML representation of `[appearance]` (UI theme prefs). Mirrors the domain
/// `Appearance` but with snake_case keys to match the rest of `app.toml`; the
/// enum *values* stay camelCase (e.g. `paperInk`) to match the renderer's set.
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AppearanceToml {
    pub mode: Option<ThemeMode>,
    pub palette_preset: Option<PalettePresetId>,
    pub density: Option<UiDensity>,
    pub accent_hue: Option<u32>,
    pub glass_theme: Option<GlassTheme>,
}

impl AppearanceToml {
    fn to_appearance(&self) -> Appearance {
        Appearance {
            mode: self.mode,
            palette_preset: self.palette_preset,
            density: self.density,
            accent_hue: self.accent_hue,
            glass_theme: self.glass_theme.clone(),
        }
    }

    fn from_appearance(appearance: &Appearance) -> Self {
        Self {
            mode: appearance.mode,
            palette_preset: appearance.palette_preset,
            density: appearance.density,
            accent_hue: appearance.accent_hue,
            glass_theme: appearance.glass_theme.clone(),
        }
    }
}

/// Daemon-specific settings read only at startup (bind address, CORS, poll
/// interval).
///
/// @spec docs/L1-accounts#apptoml
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct DaemonToml {
    pub bind: Option<String>,
    pub cors_origin: Option<String>,
    pub poll_interval_seconds: Option<u64>,
    pub require_auth: Option<bool>,
    #[serde(default, skip_serializing_if = "DaemonRuntimeTuning::is_default")]
    pub runtime: DaemonRuntimeTuning,
}

impl AppToml {
    /// Converts this TOML struct to the domain `AppSettings`.
    ///
    /// @spec docs/L1-accounts#toml-schema
    pub fn to_app_settings(&self) -> Result<AppSettings, String> {
        Ok(AppSettings {
            default_account_id: self.default_source_id.as_deref().map(AccountId::from),
            cache_policy: self.cache.to_cache_policy(),
            automation_rules: self
                .automations
                .iter()
                .map(convert_automation_rule)
                .collect::<Result<Vec<_>, _>>()?,
            automation_drafts: self
                .draft_automations
                .iter()
                .map(convert_automation_rule)
                .collect::<Result<Vec<_>, _>>()?,
            appearance: self.appearance.as_ref().map(|a| a.to_appearance()),
        })
    }

    /// Builds an `AppToml` from domain settings, preserving daemon config from
    /// the existing file.
    ///
    /// @spec docs/L1-accounts#toml-schema
    pub fn from_app_settings(settings: &AppSettings, existing: &AppToml) -> Self {
        Self {
            schema_version: existing.schema_version.max(1),
            default_source_id: settings
                .default_account_id
                .as_ref()
                .map(|id| id.to_string()),
            automations: settings
                .automation_rules
                .iter()
                .map(convert_automation_rule_to_toml)
                .collect(),
            draft_automations: settings
                .automation_drafts
                .iter()
                .map(convert_automation_rule_to_toml)
                .collect(),
            daemon: existing.daemon.clone(),
            logging: existing.logging.clone(),
            cache: CachePolicyToml::from_cache_policy(&settings.cache_policy),
            appearance: settings
                .appearance
                .as_ref()
                .map(AppearanceToml::from_appearance),
            link: existing.link.clone(),
        }
    }
}
