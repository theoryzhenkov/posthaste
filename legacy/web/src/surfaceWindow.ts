import type { SurfaceDescriptor } from './surfaces'
import { surfaceRoute } from './surfaces'

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
