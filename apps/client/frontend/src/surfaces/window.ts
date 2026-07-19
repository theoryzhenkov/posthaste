/**
 * The standalone surface window as the WEB runtime opens it: the URL a popup
 * loads, the per-surface title/size policy, and the `window.open` feature
 * string derived from it. The desktop runtime sizes real OS windows from the
 * same descriptors on the Rust side.
 */
import { surfaceRoute, type SurfaceDescriptor } from '../domain/surface/index'

interface SurfacePopupSize {
  width: number
  height: number
}

export interface SurfaceWindowPolicy {
  title: string
  popupSize: SurfacePopupSize
}

export function surfaceWindowPolicy(
  surface: SurfaceDescriptor,
): SurfaceWindowPolicy {
  switch (surface.kind) {
    case 'attachment':
      return {
        title: 'Attachment',
        popupSize: { width: 1100, height: 820 },
      }
    case 'settings':
      return {
        title: 'Settings',
        popupSize: { width: 980, height: 720 },
      }
    case 'message':
      return {
        title: 'Message',
        popupSize: { width: 900, height: 760 },
      }
    case 'compose':
      return {
        title: 'Compose',
        popupSize: { width: 780, height: 640 },
      }
  }
}

export function surfaceWindowUrl(
  location: Location,
  surface: SurfaceDescriptor,
): string {
  const url = new URL(location.href)
  url.pathname = '/'
  url.search = ''
  url.hash = surfaceRoute(surface)
  return url.toString()
}

export function surfacePopupFeatures(surface: SurfaceDescriptor): string {
  const { width, height } = surfaceWindowPolicy(surface).popupSize
  return [
    'popup',
    `width=${width}`,
    `height=${height}`,
    'resizable=yes',
    'scrollbars=yes',
  ].join(',')
}
