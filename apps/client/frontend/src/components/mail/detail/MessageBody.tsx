import { useMemo } from 'react'

import type { MessageAttachment, MessageDetail } from '@/data/transport/api'
import { useBlobUrl } from '@/data/transport/blobs'
import { resolveMessageBodyRender } from '@/data/models/messageBody'

import { EmailFrame } from './EmailFrame'
import { MESSAGE_SCROLL_ATTRIBUTE } from './model'

/** Applied to each mode's scroll container so keyboard paging can find it. */
const scrollRegion = { [MESSAGE_SCROLL_ATTRIBUTE]: '' }

/**
 * Point `cid:` image references at the blob of the inline attachment that
 * carries the matching content id; unmatched references are left alone.
 */
function resolveCidImages(
  html: string,
  attachments: MessageAttachment[],
  blobUrl: (blobId: string) => string,
): string {
  if (typeof document === 'undefined' || !html.includes('cid:')) {
    return html
  }
  const byCid = new Map(
    attachments
      .filter((attachment) => attachment.cid !== null)
      .map((attachment) => [attachment.cid as string, attachment.blobId]),
  )
  if (byCid.size === 0) {
    return html
  }
  const template = document.createElement('template')
  template.innerHTML = html
  for (const image of template.content.querySelectorAll('img[src^="cid:"]')) {
    const cid = decodeURIComponent(
      (image.getAttribute('src') ?? '').slice('cid:'.length),
    )
    const blobId = byCid.get(cid) ?? byCid.get(`<${cid}>`)
    if (blobId) {
      image.setAttribute('src', blobUrl(blobId))
    }
  }
  return template.innerHTML
}

export function MessageBody({ message }: { message: MessageDetail }) {
  const blobUrl = useBlobUrl()
  // The sanitized bodies arrive inline on the detail answer; rendering picks
  // html, then text, then the preview fallback.
  const bodyHtml = useMemo(
    () =>
      message.bodyHtml === null
        ? null
        : resolveCidImages(message.bodyHtml, message.attachments, blobUrl),
    [message.bodyHtml, message.attachments, blobUrl],
  )
  const bodyRender = resolveMessageBodyRender({
    bodyHtml,
    bodyText: message.bodyText,
    preview: message.preview,
  })

  if (bodyRender.kind === 'html') {
    return (
      <div
        className="ph-scroll h-full overflow-y-auto px-[22px] py-[18px]"
        {...scrollRegion}
      >
        <EmailFrame className="bg-transparent" html={bodyRender.html} />
      </div>
    )
  }

  if (bodyRender.kind === 'text') {
    return (
      <article
        className="ph-scroll h-full overflow-y-auto px-[22px] py-[18px] text-[13px] leading-[1.6] text-foreground/92"
        {...scrollRegion}
      >
        {bodyRender.paragraphs.map((paragraph, index) => (
          <p
            key={`${index}-${paragraph.slice(0, 20)}`}
            className="mb-4 whitespace-pre-wrap last:mb-0"
          >
            {paragraph}
          </p>
        ))}
      </article>
    )
  }

  return (
    <p className="ph-scroll h-full overflow-auto px-[22px] py-[18px] text-[13px] text-muted-foreground">
      {bodyRender.fallback}
    </p>
  )
}
