import { createContext } from 'react'
import type {
  AppliedRootTheme,
  GlassBloomId,
  GlassBloomPatch,
  PalettePresetId,
  ThemeMode,
  UiDensity,
} from '@/design'

export interface DesignThemeContextValue extends AppliedRootTheme {
  addGlassBloom: (patch?: GlassBloomPatch) => GlassBloomId
  removeGlassBloom: (bloomId: GlassBloomId) => void
  setAccentHue: (hue: number) => void
  setGlassBloom: (bloomId: GlassBloomId, patch: GlassBloomPatch) => void
  setDensity: (density: UiDensity) => void
  setMode: (mode: ThemeMode) => void
  setPalettePreset: (preset: PalettePresetId) => void
}

export const DesignThemeContext = createContext<DesignThemeContextValue | null>(
  null,
)
