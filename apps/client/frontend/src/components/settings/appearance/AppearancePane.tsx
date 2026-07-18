import { useDesignTheme } from '@/lib/design/useDesignTheme'

import { SettingsPage, SettingsPageHeader } from '../panel/shared'
import { GlassMeshEditor } from './GlassMeshEditor'
import {
  ColorsSection,
  DensitySection,
  ModeSection,
  ThemeSection,
} from './ThemeSections'

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
