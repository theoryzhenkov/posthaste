import type { ComposeForm } from '../composeFormHelpers'
import {
  MAX_COMPOSE_ATTACHMENT_BYTES,
  MAX_COMPOSE_ATTACHMENTS,
  MAX_COMPOSE_TOTAL_ATTACHMENT_BYTES,
} from '../composeFormHelpers'

export function formatFileSize(size: number): string {
  if (size < 1024) {
    return `${size} B`
  }
  if (size < 1024 * 1024) {
    return `${(size / 1024).toFixed(1)} KB`
  }
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
}

export function validateAttachmentLimits(
  attachments: ComposeForm['attachments'],
): string | null {
  if (attachments.length > MAX_COMPOSE_ATTACHMENTS) {
    return `Attach at most ${MAX_COMPOSE_ATTACHMENTS} files.`
  }
  const oversized = attachments.find(
    (attachment) => attachment.size > MAX_COMPOSE_ATTACHMENT_BYTES,
  )
  if (oversized) {
    return `${oversized.filename} is larger than ${formatFileSize(MAX_COMPOSE_ATTACHMENT_BYTES)}.`
  }
  const totalSize = attachments.reduce(
    (total, attachment) => total + attachment.size,
    0,
  )
  if (totalSize > MAX_COMPOSE_TOTAL_ATTACHMENT_BYTES) {
    return `Attachments can total at most ${formatFileSize(MAX_COMPOSE_TOTAL_ATTACHMENT_BYTES)}.`
  }
  return null
}
