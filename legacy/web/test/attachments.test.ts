import { describe, expect, it } from 'bun:test'

import type { MessageAttachment } from '../src/api/types'
import { canPreviewAttachment, formatAttachmentSize } from '../src/attachments'

function attachment(mimeType: string): MessageAttachment {
  return { mimeType } as MessageAttachment
}

describe('attachment helpers', () => {
  it('previews images, PDFs, and text but not arbitrary binaries', () => {
    expect(canPreviewAttachment(attachment('image/png'))).toBe(true)
    expect(canPreviewAttachment(attachment('image/jpeg'))).toBe(true)
    expect(canPreviewAttachment(attachment('application/pdf'))).toBe(true)
    expect(canPreviewAttachment(attachment('text/plain'))).toBe(true)
    expect(canPreviewAttachment(attachment('text/calendar'))).toBe(true)
    expect(canPreviewAttachment(attachment('application/zip'))).toBe(false)
    expect(canPreviewAttachment(attachment('application/octet-stream'))).toBe(
      false,
    )
  })

  it('formats size in B/KB/MB with boundaries at 1024', () => {
    expect(formatAttachmentSize(0)).toBe('0 B')
    expect(formatAttachmentSize(1023)).toBe('1023 B')
    expect(formatAttachmentSize(1024)).toBe('1.0 KB')
    expect(formatAttachmentSize(1536)).toBe('1.5 KB')
    expect(formatAttachmentSize(1024 * 1024 - 1)).toBe('1024.0 KB')
    expect(formatAttachmentSize(1024 * 1024)).toBe('1.0 MB')
    expect(formatAttachmentSize(1024 * 1024 * 1.5)).toBe('1.5 MB')
  })
})
