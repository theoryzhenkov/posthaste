export const designStorageKeys = {
  themeMode: "posthaste.themeMode",
  palettePreset: "posthaste.palettePreset",
  uiDensity: "posthaste.uiDensity",
} as const;

export type DesignStorageKeyName = keyof typeof designStorageKeys;
export type DesignStorageKey = (typeof designStorageKeys)[DesignStorageKeyName];

export const designDataAttributes = {
  themeMode: "data-theme-mode",
  resolvedThemeMode: "data-resolved-theme-mode",
  palettePreset: "data-palette-preset",
  paletteStyle: "data-palette-style",
  uiDensity: "data-ui-density",
} as const;

export type DesignDataAttributeName = keyof typeof designDataAttributes;
export type DesignDataAttribute =
  (typeof designDataAttributes)[DesignDataAttributeName];

export const designClassNames = {
  dark: "dark",
} as const;

export type DesignClassName = (typeof designClassNames)[keyof typeof designClassNames];
