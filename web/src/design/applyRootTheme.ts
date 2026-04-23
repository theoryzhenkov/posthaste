import { designClassNames, designDataAttributes } from "./attributes";
import { defaultUiDensity, type UiDensity } from "./density";
import {
  defaultPalettePresetId,
  defaultThemeMode,
  palettePresets,
  resolvePaletteMode,
  type PalettePresetId,
  type ResolvedThemeMode,
  type ThemeMode,
} from "./theme";

export type RootThemeState = {
  readonly mode?: ThemeMode;
  readonly palettePreset?: PalettePresetId;
  readonly density?: UiDensity;
};

export type AppliedRootTheme = {
  readonly mode: ThemeMode;
  readonly resolvedMode: ResolvedThemeMode;
  readonly palettePreset: PalettePresetId;
  readonly density: UiDensity;
};

export function resolveThemeMode(
  mode: ThemeMode,
  systemMode: ResolvedThemeMode,
): ResolvedThemeMode {
  return mode === "system" ? systemMode : mode;
}

export function getSystemThemeMode(
  matchMedia: Window["matchMedia"] | undefined = globalThis.matchMedia,
): ResolvedThemeMode {
  if (matchMedia?.("(prefers-color-scheme: dark)").matches) {
    return "dark";
  }
  return "light";
}

export function applyRootTheme(
  root: HTMLElement,
  state: RootThemeState,
  systemMode: ResolvedThemeMode = getSystemThemeMode(),
): AppliedRootTheme {
  const mode = state.mode ?? defaultThemeMode;
  const palettePreset = state.palettePreset ?? defaultPalettePresetId;
  const density = state.density ?? defaultUiDensity;
  const requestedMode = resolveThemeMode(mode, systemMode);
  const resolvedMode = resolvePaletteMode(palettePreset, requestedMode);
  const palette = palettePresets[palettePreset];

  root.setAttribute(designDataAttributes.themeMode, mode);
  root.setAttribute(designDataAttributes.resolvedThemeMode, resolvedMode);
  root.setAttribute(designDataAttributes.palettePreset, palettePreset);
  root.setAttribute(designDataAttributes.paletteStyle, palette.style);
  root.setAttribute(designDataAttributes.uiDensity, density);
  root.classList.toggle(designClassNames.dark, resolvedMode === "dark");

  return {
    mode,
    resolvedMode,
    palettePreset,
    density,
  };
}
