use serde::{Deserialize, Serialize};

/// App-level appearance/theme preferences — the renderer's visual settings,
/// stored in `[appearance]` of `app.toml` as the single source of truth (moved
/// out of the opaque `localStorage` "client-preferences" snapshot).
///
/// The backend treats appearance as **pass-through storage**: it does not
/// interpret theme values (the renderer applies them). The enums give the
/// OpenAPI schema self-documentation and reject typos at the parse boundary.
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Appearance {
    pub mode: Option<ThemeMode>,
    pub palette_preset: Option<PalettePresetId>,
    pub density: Option<UiDensity>,
    pub accent_hue: Option<u32>,
    pub glass_theme: Option<GlassTheme>,
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

/// Palette preset identifier (the renderer resolves these to concrete palettes).
/// Matches the renderer's `PalettePresetId` set; extend both together when a
/// preset is added.
///
/// @spec docs/eph/DESIGN-L2-appearance-toml
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum PalettePresetId {
    Neutral,
    PaperInk,
    Brutalist,
    Glass,
    Acid,
    Marzipan,
    Botanical,
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
