import { useDesignTheme } from '@/hooks/useDesignTheme'

import { SettingsPage, SettingsPageHeader } from './shared'
import { GlassMeshEditor } from './appearance/GlassMeshEditor'
import {
  ColorsSection,
  DensitySection,
  ModeSection,
  ThemeSection,
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
        <ThemeSection theme={theme} />
        <ColorsSection theme={theme} />
        {theme.theme === 'glass' && <GlassMeshEditor />}
        <ModeSection theme={theme} />
        <DensitySection theme={theme} />
      </div>
    </SettingsPage>
  )
}
