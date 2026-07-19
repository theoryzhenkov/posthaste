export const designStorageKeys = {
  themeMode: 'posthaste.themeMode.v3',
  theme: 'posthaste.theme.v1',
  uiDensity: 'posthaste.uiDensity.v3',
  themeColors: 'posthaste.themeColors.v1',
  themeParameters: 'posthaste.themeParameters.v1',
} as const

export const designDataAttributes = {
  themeMode: 'data-theme-mode',
  resolvedThemeMode: 'data-resolved-theme-mode',
  palettePreset: 'data-palette-preset',
  paletteStyle: 'data-palette-style',
  uiDensity: 'data-ui-density',
} as const

export const designClassNames = {
  dark: 'dark',
} as const
