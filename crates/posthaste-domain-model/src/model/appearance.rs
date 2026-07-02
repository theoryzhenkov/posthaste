use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// App-level appearance/theme preferences — the renderer's visual settings,
/// stored in `[appearance]` of `app.toml` as the single source of truth (moved
/// out of the opaque `localStorage` "client-preferences" snapshot).
///
/// The authority server treats appearance as **pass-through storage**: it does not
/// interpret theme values (the renderer applies them). The enums give the
/// OpenAPI schema self-documentation and reject typos at the parse boundary.
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Appearance {
    pub mode: Option<ThemeMode>,
    /// Theme identifier. Free-form (not an enum) so user-created themes are
    /// expressible without a schema change; the built-ins are `"neutral"`
    /// (displayed "Classic") and `"glass"`. Pass-through: the renderer resolves
    /// the id to a palette and treats an unknown id as the default.
    #[serde(alias = "palettePreset")]
    pub theme: Option<String>,
    pub density: Option<UiDensity>,
    /// Per-mode color overrides. Light/dark are customized independently (a
    /// legacy single `accent_hue` in an older `app.toml` seeds both).
    pub light: Option<ThemeColors>,
    pub dark: Option<ThemeColors>,
    pub glass_theme: Option<GlassTheme>,
}

/// Per-mode color overrides for a theme. The named knobs (`accent_hue`,
/// `surface_hue`) are the curated UX; `tokens` is the open escape hatch.
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ThemeColors {
    /// Accent color hue (0–360).
    pub accent_hue: Option<u32>,
    /// Base/surface color hue (0–360) — the "main color", today a fixed grey.
    pub surface_hue: Option<u32>,
    /// Arbitrary CSS custom-property overrides (`--token` → value). The
    /// foundation for user-supplied themes / imported CSS: a future loader
    /// parses a `.css` file into this map. Pass-through storage — the renderer
    /// applies recognized tokens and ignores the rest; absent today's UI.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, String>,
}

/// Theme mode: light, dark, or follow the OS preference.
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

/// UI density (information density of the layout).
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum UiDensity {
    Compact,
    Cozy,
    Comfortable,
}

/// Advanced glass-theme parameters: a set of decorative "blooms" rendered as the
/// background. Pass-through storage; the renderer normalizes/clamps values.
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GlassTheme {
    pub blooms: Vec<GlassBloom>,
}

/// One decorative bloom in a [`GlassTheme`].
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GlassBloom {
    pub id: String,
    pub hue: u32,
    pub x: f64,
    pub y: f64,
    pub opacity: f64,
    pub radius: f64,
}
