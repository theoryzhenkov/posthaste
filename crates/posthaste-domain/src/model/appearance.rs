//! Global UI appearance preferences (theme, palette, density, glass effects).
//!
//! Extracted from the parent `model` module to keep presentation-settings types grouped.

use serde::{Deserialize, Serialize};

/// Global UI appearance preferences shared across app windows.
///
/// @spec docs/L1-accounts#toml-schema
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AppAppearanceSettings {
    #[serde(default)]
    pub mode: AppThemeMode,
    #[serde(default)]
    pub palette_preset: AppPalettePreset,
    #[serde(default)]
    pub density: AppUiDensity,
    #[serde(default = "default_accent_hue")]
    pub accent_hue: u16,
    #[serde(default)]
    pub glass_theme: AppGlassThemeSettings,
}

impl Default for AppAppearanceSettings {
    fn default() -> Self {
        Self {
            mode: AppThemeMode::default(),
            palette_preset: AppPalettePreset::default(),
            density: AppUiDensity::default(),
            accent_hue: default_accent_hue(),
            glass_theme: AppGlassThemeSettings::default(),
        }
    }
}

fn default_accent_hue() -> u16 {
    45
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AppThemeMode {
    Light,
    #[default]
    Dark,
    System,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AppPalettePreset {
    #[default]
    Neutral,
    PaperInk,
    Brutalist,
    Glass,
    Acid,
    Marzipan,
    Botanical,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AppUiDensity {
    #[default]
    Compact,
    Cozy,
    Comfortable,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AppGlassThemeSettings {
    #[serde(default = "default_glass_blooms")]
    pub blooms: Vec<AppGlassBloomSettings>,
}

impl Default for AppGlassThemeSettings {
    fn default() -> Self {
        Self {
            blooms: default_glass_blooms(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AppGlassBloomSettings {
    pub id: String,
    pub hue: u16,
    pub x: f64,
    pub y: f64,
    pub opacity: f64,
    pub radius: f64,
}

fn default_glass_blooms() -> Vec<AppGlassBloomSettings> {
    vec![
        AppGlassBloomSettings {
            id: "bloom-1".to_string(),
            hue: 285,
            x: 20.0,
            y: 10.0,
            opacity: 0.35,
            radius: 45.0,
        },
        AppGlassBloomSettings {
            id: "bloom-2".to_string(),
            hue: 345,
            x: 85.0,
            y: 25.0,
            opacity: 0.25,
            radius: 45.0,
        },
        AppGlassBloomSettings {
            id: "bloom-3".to_string(),
            hue: 220,
            x: 50.0,
            y: 90.0,
            opacity: 0.3,
            radius: 50.0,
        },
        AppGlassBloomSettings {
            id: "bloom-4".to_string(),
            hue: 155,
            x: 10.0,
            y: 85.0,
            opacity: 0.2,
            radius: 40.0,
        },
    ]
}
