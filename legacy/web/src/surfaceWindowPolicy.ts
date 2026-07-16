import type { SurfaceDescriptor } from './surfaces'

export interface SurfacePopupSize {
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
