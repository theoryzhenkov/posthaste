import { createContext } from "react";
import type {
  AppliedRootTheme,
  PalettePresetId,
  ThemeMode,
  UiDensity,
} from "@/design";

export interface DesignThemeContextValue extends AppliedRootTheme {
  setDensity: (density: UiDensity) => void;
  setMode: (mode: ThemeMode) => void;
  setPalettePreset: (preset: PalettePresetId) => void;
}

export const DesignThemeContext =
  createContext<DesignThemeContextValue | null>(null);
