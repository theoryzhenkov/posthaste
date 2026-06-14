import { Forward, Mail, Reply } from 'lucide-react'

import type { ComposeIntent } from '@/composeIntent'

export function ComposeHeader({
  fromLabel,
  intentKind,
}: {
  fromLabel: string
  intentKind: ComposeIntent['kind']
}) {
  return (
    <div className="flex h-11 min-w-0 items-center gap-2 px-3">
      <div className="flex size-7 shrink-0 items-center justify-center rounded-[7px] bg-[color-mix(in_oklab,var(--brand-coral)_12%,transparent)] text-muted-foreground">
        {intentKind === 'reply' ? (
          <Reply size={15} />
        ) : intentKind === 'forward' ? (
          <Forward size={15} />
        ) : (
          <Mail size={15} />
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-semibold">
          {intentKind === 'reply'
            ? 'Reply'
            : intentKind === 'forward'
              ? 'Forward'
              : 'New Message'}
        </div>
        <div className="truncate text-[11px] text-muted-foreground">
          {fromLabel}
        </div>
      </div>
    </div>
  )
}
