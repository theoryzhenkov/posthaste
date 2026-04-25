import type { MailSelection } from './mailState'

export type SurfaceDisposition = 'focused'

export interface MessageSurfaceDescriptor {
  kind: 'message'
  disposition: SurfaceDisposition
  params: {
    conversationId: string
    sourceId: string
    messageId: string
  }
}

export type SurfaceDescriptor = MessageSurfaceDescriptor

export function messageSurfaceFromSelection(
  selection: MailSelection,
): MessageSurfaceDescriptor {
  return {
    kind: 'message',
    disposition: 'focused',
    params: {
      conversationId: selection.conversationId,
      sourceId: selection.sourceId,
      messageId: selection.messageId,
    },
  }
}

export function surfaceRoute(surface: SurfaceDescriptor): string {
  const params = new URLSearchParams(surface.params)
  return `/surface/${surface.kind}?${params.toString()}`
}
