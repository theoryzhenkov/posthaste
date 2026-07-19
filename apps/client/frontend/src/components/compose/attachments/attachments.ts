import type { ComposeForm } from '../form/model'
import { formatAttachmentSize } from '@/data/models/attachments'
import {
  MAX_COMPOSE_ATTACHMENT_BYTES,
  MAX_COMPOSE_ATTACHMENTS,
  MAX_COMPOSE_TOTAL_ATTACHMENT_BYTES,
} from '../form/model'

/**
 * Extract the `File`s carried by a paste or drag-and-drop `DataTransfer`.
 * Returns an empty array for text-only payloads, so callers can leave plain
 * text pastes/drops entirely to the default handling.
 */
export function filesFromDataTransfer(data: DataTransfer | null): File[] {
  if (!data) {
    return []
  }
  if (data.files && data.files.length > 0) {
    return Array.from(data.files)
  }
  // Some clipboard sources only expose items (kind === 'file'), not `files`.
  const files: File[] = []
  for (const item of Array.from(data.items ?? [])) {
    if (item.kind === 'file') {
      const file = item.getAsFile()
      if (file) {
        files.push(file)
      }
    }
  }
  return files
}

/**
 * Give a clipboard file without a name (typical for a pasted screenshot) a
 * generated one — `pasted-image-<n>.<ext>` for images (extension from the
 * MIME subtype), `pasted-file-<n>` otherwise — preserving the MIME type.
 * Named files pass through untouched.
 */
export function withPastedFileName(file: File, ordinal: number): File {
  if (file.name) {
    return file
  }
  const mimeType = file.type || 'application/octet-stream'
  const name = mimeType.startsWith('image/')
    ? `pasted-image-${ordinal}.${mimeType.slice('image/'.length).split('+')[0] || 'png'}`
    : `pasted-file-${ordinal}`
  return new File([file], name, {
    type: mimeType,
    lastModified: file.lastModified,
  })
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
    return `${oversized.filename} is larger than ${formatAttachmentSize(MAX_COMPOSE_ATTACHMENT_BYTES)}.`
  }
  const totalSize = attachments.reduce(
    (total, attachment) => total + attachment.size,
    0,
  )
  if (totalSize > MAX_COMPOSE_TOTAL_ATTACHMENT_BYTES) {
    return `Attachments can total at most ${formatAttachmentSize(MAX_COMPOSE_TOTAL_ATTACHMENT_BYTES)}.`
  }
  return null
}
