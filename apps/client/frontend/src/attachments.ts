import type { MessageAttachment } from './api/types'

export function canPreviewAttachment(attachment: MessageAttachment): boolean {
  return (
    attachment.mimeType.startsWith('image/') ||
    attachment.mimeType === 'application/pdf' ||
    attachment.mimeType.startsWith('text/')
  )
}

export function formatAttachmentSize(size: number): string {
  if (size < 1024) {
    return `${size} B`
  }
  if (size < 1024 * 1024) {
    return `${(size / 1024).toFixed(1)} KB`
  }
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}
