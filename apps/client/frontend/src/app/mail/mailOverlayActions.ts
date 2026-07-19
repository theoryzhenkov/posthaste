import { closeWebSurface } from '@/surfaces/navigation'
import { openFocusedSurface } from '../host/navigation'
import { settingsSurface, type SurfaceDescriptor } from '@/domain/surface'

export function toggleSettingsSurface(input: {
  effectiveSurface: SurfaceDescriptor | null
}) {
  if (input.effectiveSurface?.kind === 'settings') {
    closeWebSurface()
  } else {
    openFocusedSurface(settingsSurface())
  }
}
