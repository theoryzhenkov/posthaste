import { closeWebSurface, openFocusedSurface } from '@/hooks/useSurfaceRouting'
import { settingsSurface, type SurfaceDescriptor } from '@/surfaces'

export function toggleSettingsSurface(input: {
  effectiveSurface: SurfaceDescriptor | null
  shouldRenderForcedSettings: boolean
}) {
  if (
    input.effectiveSurface?.kind === 'settings' &&
    !input.shouldRenderForcedSettings
  ) {
    closeWebSurface()
  } else {
    openFocusedSurface(settingsSurface())
  }
}
