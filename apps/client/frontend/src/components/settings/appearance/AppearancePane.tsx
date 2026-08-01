import { useDesignTheme } from '@/lib/design/useDesignTheme'

import { SettingsPage, SettingsPageHeader } from '../panel/shared'
import { GlassMeshEditor } from './GlassMeshEditor'
import { MessageFieldsSection } from './MessageFieldsSection'
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
        description="Choose the built-in theme, color mode, interface density, and which fields a message shows."
      />

      <div>
        <ThemeSection theme={theme} />
        <ColorsSection theme={theme} />
        {theme.theme === 'glass' && <GlassMeshEditor />}
        <ModeSection theme={theme} />
        <DensitySection theme={theme} />
        <MessageFieldsSection />
      </div>
    </SettingsPage>
  )
}
