import { createContext } from 'react'
import type {
  AppliedRootTheme,
  GlassBloomId,
  GlassBloomPatch,
  ResolvedThemeMode,
  ThemeMode,
  UiDensity,
} from '@/lib/design'

export interface DesignThemeContextValue extends AppliedRootTheme {
  addGlassBloom: (patch?: GlassBloomPatch) => GlassBloomId
  removeGlassBloom: (bloomId: GlassBloomId) => void
  /** Set the accent hue for one mode (light/dark are edited independently). */
  setAccentHue: (mode: ResolvedThemeMode, hue: number) => void
  /** Set the surface ("main color") hue for one mode. */
  setSurfaceHue: (mode: ResolvedThemeMode, hue: number) => void
  setGlassBloom: (bloomId: GlassBloomId, patch: GlassBloomPatch) => void
  setDensity: (density: UiDensity) => void
  setMode: (mode: ThemeMode) => void
  setTheme: (theme: string) => void
}

export const DesignThemeContext = createContext<DesignThemeContextValue | null>(
  null,
)
