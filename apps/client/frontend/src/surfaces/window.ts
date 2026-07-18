import type { SurfaceDescriptor } from './index'
import { surfaceRoute } from './index'

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
