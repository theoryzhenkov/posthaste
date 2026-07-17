import { closeWebSurface, openFocusedSurface } from '@/hooks/useSurfaceRouting'
import { settingsSurface, type SurfaceDescriptor } from '@/surfaces'

export function toggleSettingsSurface(input: {
  effectiveSurface: SurfaceDescriptor | null
}) {
  if (input.effectiveSurface?.kind === 'settings') {
    closeWebSurface()
  } else {
    openFocusedSurface(settingsSurface())
  }
}
