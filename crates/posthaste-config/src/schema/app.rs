use super::*;

use posthaste_domain_model::ComposeSettings;
use std::collections::HashMap;

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
    pub notifications: Option<NotificationsToml>,
    #[serde(default, rename = "mailbox_colors")]
    pub mailbox_colors: Vec<MailboxColorToml>,
    /// Per-tag presentation overrides (`[[tags]]`).
    #[serde(default)]
    pub tags: Vec<TagAppearanceToml>,
    /// User's explicit sidebar arrangement (ids). Override lists; absent ids
    /// fall back to the canonical/default order at read time.
    #[serde(default)]
    pub smart_mailbox_order: Vec<String>,
    #[serde(default)]
    pub account_order: Vec<String>,
    /// Client-side sidebar Groups (`[[mailbox_groups]]`). Presentation only.
    #[serde(default, rename = "mailbox_groups")]
    pub mailbox_groups: Vec<MailboxGroupToml>,
    /// Compose/sending preferences (`[compose]`, e.g. the undo-send delay).
    #[serde(default)]
    pub compose: Option<ComposeToml>,
    #[serde(default)]
    pub link: LinkToml,
    /// Optional in-daemon TLS (`[tls]` cert+key). When present the daemon serves
    /// HTTPS over the bound address; absent = plaintext loopback (the default
    /// bundled behavior).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls: Option<TlsToml>,
}

/// TOML representation of a per-mailbox color override (`[[mailbox_colors]]`).
///
/// @spec docs/eph/RFC-L2-configuration-matrix
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MailboxColorToml {
    pub source_id: String,
    pub mailbox_id: String,
    pub hue: u32,
}

impl MailboxColorToml {
    fn to_mailbox_color(&self) -> MailboxColor {
        MailboxColor {
            source_id: AccountId::from(self.source_id.as_str()),
            mailbox_id: MailboxId::from(self.mailbox_id.as_str()),
            hue: self.hue,
        }
    }

    fn from_mailbox_color(color: &MailboxColor) -> Self {
        Self {
            source_id: color.source_id.to_string(),
            mailbox_id: color.mailbox_id.as_str().to_string(),
            hue: color.hue,
        }
    }
}

/// TOML representation of a client-side sidebar Group (`[[mailbox_groups]]`).
/// Presentation only — never maps to a provider mailbox.
///
/// @spec docs/eph/RFC-L2-mailbox-management#a4
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct MailboxGroupToml {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mailbox_ids: Vec<String>,
    pub order: i64,
}

impl MailboxGroupToml {
    fn to_mailbox_group(&self) -> MailboxGroup {
        MailboxGroup {
            id: self.id.clone(),
            name: self.name.clone(),
            mailbox_ids: self.mailbox_ids.clone(),
            order: self.order,
        }
    }

    fn from_mailbox_group(group: &MailboxGroup) -> Self {
        Self {
            id: group.id.clone(),
            name: group.name.clone(),
            mailbox_ids: group.mailbox_ids.clone(),
            order: group.order,
        }
    }
}

/// TOML representation of a per-tag presentation override (`[[tags]]`).
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TagAppearanceToml {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
}

impl TagAppearanceToml {
    fn to_tag_appearance(&self) -> TagAppearance {
        TagAppearance {
            name: self.name.clone(),
            fg: self.fg.clone(),
            bg: self.bg.clone(),
            icon: self.icon.clone(),
        }
    }

    fn from_tag_appearance(tag: &TagAppearance) -> Self {
        Self {
            name: tag.name.clone(),
            fg: tag.fg.clone(),
            bg: tag.bg.clone(),
            icon: tag.icon.clone(),
        }
    }
}

/// Runtime↔authority server link settings (`[link]`). Default: in-process, link not
/// served — the bundled single-process deployment is unaffected.
///
/// @spec docs/replication/L1#10-deployment-topology
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LinkToml {
    /// Authority server role: serve the runtime↔authority-server link over HTTP for a remote
    /// runtime. Default `false`.
    pub serve: Option<bool>,
    /// Connect role: this near node's bearer token, presented to the remote
    /// authority server (single token — the near node is one runtime). The authority server
    /// resolves it to a `AuthorityServerLinkId` via [`runtimes`](Self::runtimes) on the
    /// serve side.
    pub token: Option<String>,
    /// Serve role: the runtimes permitted to connect, as `token → runtime_id`
    /// (X runtimes, X ≥ 1; no single-runtime special case). Required under
    /// `serve` + `require_auth`. A connecting runtime presents its [`token`];
    /// the authority server looks it up here to derive its identity.
    pub runtimes: Option<HashMap<String, String>>,
    /// Runtime role: connect to a remote authority server at this base URL instead of the
    /// in-process one. When set, this process is a near node over the link.
    pub authority_server_url: Option<String>,
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
/// `Appearance` with snake_case keys to match the rest of `app.toml`.
///
/// Back-compat: an older file's `palette_preset` reads into `theme` (serde
/// alias) and a legacy top-level `accent_hue` seeds both modes when `light`/
/// `dark` are absent. Neither legacy key is written back — a save re-serializes
/// the current per-mode shape, migrating the file in place.
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AppearanceToml {
    pub mode: Option<ThemeMode>,
    #[serde(alias = "palette_preset", skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    pub density: Option<UiDensity>,
    /// Legacy single accent (pre per-mode). Read-only: seeds `light`/`dark`.
    #[serde(skip_serializing)]
    pub accent_hue: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub light: Option<ThemeColorsToml>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dark: Option<ThemeColorsToml>,
    pub glass_theme: Option<GlassTheme>,
}

impl AppearanceToml {
    fn to_appearance(&self) -> Appearance {
        // A legacy top-level accent seeds both modes when neither is set.
        let legacy = self.accent_hue.map(|accent_hue| ThemeColors {
            accent_hue: Some(accent_hue),
            ..ThemeColors::default()
        });
        Appearance {
            mode: self.mode,
            theme: self.theme.clone(),
            density: self.density,
            light: self
                .light
                .as_ref()
                .map(ThemeColorsToml::to_theme_colors)
                .or_else(|| legacy.clone()),
            dark: self
                .dark
                .as_ref()
                .map(ThemeColorsToml::to_theme_colors)
                .or(legacy),
            glass_theme: self.glass_theme.clone(),
        }
    }

    fn from_appearance(appearance: &Appearance) -> Self {
        Self {
            mode: appearance.mode,
            theme: appearance.theme.clone(),
            density: appearance.density,
            accent_hue: None,
            light: appearance
                .light
                .as_ref()
                .map(ThemeColorsToml::from_theme_colors),
            dark: appearance
                .dark
                .as_ref()
                .map(ThemeColorsToml::from_theme_colors),
            glass_theme: appearance.glass_theme.clone(),
        }
    }
}

/// TOML representation of per-mode [`ThemeColors`].
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ThemeColorsToml {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accent_hue: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub surface_hue: Option<u32>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub tokens: std::collections::BTreeMap<String, String>,
}

impl ThemeColorsToml {
    fn to_theme_colors(&self) -> ThemeColors {
        ThemeColors {
            accent_hue: self.accent_hue,
            surface_hue: self.surface_hue,
            tokens: self.tokens.clone(),
        }
    }

    fn from_theme_colors(colors: &ThemeColors) -> Self {
        Self {
            accent_hue: colors.accent_hue,
            surface_hue: colors.surface_hue,
            tokens: colors.tokens.clone(),
        }
    }
}

/// TOML representation of `[notifications]` (notification policy). Mirrors the
/// domain `Notifications` but with snake_case keys to match the rest of
/// `app.toml`.
///
/// @spec docs/eph/RFC-L2-configuration-matrix
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct NotificationsToml {
    pub new_mail: Option<bool>,
    pub sound: Option<bool>,
}

impl NotificationsToml {
    fn to_notifications(&self) -> Notifications {
        Notifications {
            new_mail: self.new_mail,
            sound: self.sound,
        }
    }

    fn from_notifications(notifications: &Notifications) -> Self {
        Self {
            new_mail: notifications.new_mail,
            sound: notifications.sound,
        }
    }
}

/// TOML representation of `[compose]` (compose/sending preferences).
///
/// @spec docs/L1-accounts#apptoml
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ComposeToml {
    /// Undo-send hold in seconds; absent = the app default, `0` = no hold.
    pub undo_send_delay_seconds: Option<u32>,
}

impl ComposeToml {
    fn to_compose_settings(&self) -> ComposeSettings {
        ComposeSettings {
            undo_send_delay_seconds: self.undo_send_delay_seconds,
        }
    }

    fn from_compose_settings(compose: &ComposeSettings) -> Self {
        Self {
            undo_send_delay_seconds: compose.undo_send_delay_seconds,
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
    /// Extra hosts admitted by the `Host`-header DNS-rebinding guard beyond the
    /// loopback + bind-host defaults. Needed for remote clients over a hostname
    /// (e.g. a TLS deployment reached as `mail.host:3001`) — a wildcard
    /// `0.0.0.0` bind alone admits no external host.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_hosts: Vec<String>,
}

/// Optional `[tls]` config for in-daemon TLS. Both `cert` and `key` must be
/// present together; a partial config is rejected at startup (fail closed).
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct TlsToml {
    pub cert: Option<std::path::PathBuf>,
    pub key: Option<std::path::PathBuf>,
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
            notifications: self.notifications.as_ref().map(|n| n.to_notifications()),
            mailbox_colors: self
                .mailbox_colors
                .iter()
                .map(MailboxColorToml::to_mailbox_color)
                .collect(),
            tags: self
                .tags
                .iter()
                .map(TagAppearanceToml::to_tag_appearance)
                .collect(),
            smart_mailbox_order: self
                .smart_mailbox_order
                .iter()
                .map(|id| SmartMailboxId::from(id.as_str()))
                .collect(),
            account_order: self
                .account_order
                .iter()
                .map(|id| AccountId::from(id.as_str()))
                .collect(),
            mailbox_groups: self
                .mailbox_groups
                .iter()
                .map(MailboxGroupToml::to_mailbox_group)
                .collect(),
            compose: self.compose.as_ref().map(ComposeToml::to_compose_settings),
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
            notifications: settings
                .notifications
                .as_ref()
                .map(NotificationsToml::from_notifications),
            mailbox_colors: settings
                .mailbox_colors
                .iter()
                .map(MailboxColorToml::from_mailbox_color)
                .collect(),
            tags: settings
                .tags
                .iter()
                .map(TagAppearanceToml::from_tag_appearance)
                .collect(),
            smart_mailbox_order: settings
                .smart_mailbox_order
                .iter()
                .map(|id| id.to_string())
                .collect(),
            account_order: settings
                .account_order
                .iter()
                .map(|id| id.to_string())
                .collect(),
            mailbox_groups: settings
                .mailbox_groups
                .iter()
                .map(MailboxGroupToml::from_mailbox_group)
                .collect(),
            compose: settings
                .compose
                .as_ref()
                .map(ComposeToml::from_compose_settings),
            link: existing.link.clone(),
            // TLS is daemon-side config, never derived from AppSettings; preserve
            // whatever the existing file declared.
            tls: existing.tls.clone(),
        }
    }
}
