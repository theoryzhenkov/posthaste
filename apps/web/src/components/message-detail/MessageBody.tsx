import type { MessageDetail } from '@/api/types'
import { resolveMessageBodyRender } from '@/messageBody'

import { EmailFrame } from '../EmailFrame'

export function MessageBody({ message }: { message: MessageDetail }) {
  const bodyRender = resolveMessageBodyRender(message)

  if (bodyRender.kind === 'html') {
    return (
      <div className="ph-scroll h-full overflow-auto px-[22px] py-[18px]">
        <EmailFrame
          className="h-full min-h-[480px] bg-transparent"
          html={bodyRender.html}
        />
      </div>
    )
  }

  if (bodyRender.kind === 'text') {
    return (
      <article className="ph-scroll h-full overflow-auto px-[22px] py-[18px] text-[13px] leading-[1.6] text-foreground/92">
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
