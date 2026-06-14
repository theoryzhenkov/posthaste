import { useDesignTheme } from '@/hooks/useDesignTheme'

import { SettingsPage, SettingsPageHeader } from './shared'
import { GlassMeshEditor } from './appearance/GlassMeshEditor'
import {
  AccentSection,
  DensitySection,
  ModeSection,
  ThemePresetSection,
} from './appearance/ThemeSections'

export function AppearancePane() {
  const theme = useDesignTheme()

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Appearance"
        description="Choose the built-in theme, color mode, and interface density."
      />

      <div>
        <ThemePresetSection theme={theme} />
        <AccentSection theme={theme} />
        {theme.palettePreset === 'glass' && <GlassMeshEditor />}
        <ModeSection theme={theme} />
        <DensitySection theme={theme} />
      </div>
    </SettingsPage>
  )
}
