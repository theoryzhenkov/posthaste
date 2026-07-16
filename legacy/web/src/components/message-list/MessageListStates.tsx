import type { MouseEvent } from 'react'
import { Inbox, Loader2, MailPlus, MousePointerClick } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { openFocusedSurface } from '@/hooks/useSurfaceRouting'
import { settingsCategorySurface } from '@/surfaces'

export function NoMailboxSelected({
  onMouseDown,
  hasNoAccounts = false,
}: {
  onMouseDown: (event: MouseEvent<HTMLDivElement>) => void
  hasNoAccounts?: boolean
}) {
  // With no enabled accounts the runtime serves no view, so the list lands here
  // on first launch. That's onboarding, not a missing selection — offer a direct
  // path into account setup instead of the dead-end "pick a mailbox".
  if (hasNoAccounts) {
    return (
      <div
        className="flex h-full flex-col items-center justify-center gap-3 bg-panel p-6"
        data-message-list-empty="true"
        onMouseDown={onMouseDown}
      >
        <MailPlus
          size={40}
          strokeWidth={1.5}
          className="text-muted-foreground/40"
        />
        <div className="text-center">
          <p className="text-sm font-medium text-muted-foreground">
            No accounts yet
          </p>
          <p className="mt-1 text-xs text-muted-foreground/60">
            Add an email account to start receiving mail
          </p>
        </div>
        <Button
          size="sm"
          className="mt-1"
          onClick={() =>
            openFocusedSurface(settingsCategorySurface('accounts'))
          }
        >
          <MailPlus />
          Add an account
        </Button>
      </div>
    )
  }
  return (
    <div
      className="flex h-full flex-col items-center justify-center gap-3 bg-panel p-6"
      data-message-list-empty="true"
      onMouseDown={onMouseDown}
    >
      <MousePointerClick
        size={40}
        strokeWidth={1.5}
        className="text-muted-foreground/40"
      />
      <div className="text-center">
        <p className="text-sm font-medium text-muted-foreground">
          No mailbox selected
        </p>
        <p className="mt-1 text-xs text-muted-foreground/60">
          Pick a mailbox to get started
        </p>
      </div>
    </div>
  )
}

export function LoadingRows({ rowHeight }: { rowHeight: number }) {
  return (
    <div
      className="space-y-0 bg-[var(--list-zebra)]"
      data-message-list-empty="true"
    >
      {Array.from({ length: 4 }).map((_, i) => (
        <div
          key={i}
          className="border-b border-[var(--list-divider)] px-4 py-3"
          style={{ height: rowHeight }}
        >
          <div className="flex items-center gap-3">
            <div className="h-3.5 w-28 animate-pulse rounded bg-muted" />
            <div className="h-3 w-16 animate-pulse rounded bg-muted" />
          </div>
          <div className="mt-2.5 h-3 w-3/4 animate-pulse rounded bg-muted" />
          <div className="mt-2 h-3 w-1/2 animate-pulse rounded bg-muted/60" />
        </div>
      ))}
    </div>
  )
}

export function EmptyMessages({ isSyncing = false }: { isSyncing?: boolean }) {
  // During an initial/repair sync the projection is legitimately empty while
  // mail streams in — show a syncing state rather than a bare "no messages".
  if (isSyncing) {
    return (
      <div
        className="flex flex-col items-center gap-3 px-3 py-12"
        data-message-list-empty="true"
      >
        <Loader2
          size={32}
          strokeWidth={1.6}
          className="animate-spin text-muted-foreground/50"
        />
        <div className="text-center">
          <p className="text-sm font-medium text-muted-foreground">
            Syncing your mail…
          </p>
          <p className="mt-1 text-xs text-muted-foreground/60">
            Messages will appear as they arrive
          </p>
        </div>
      </div>
    )
  }
  return (
    <div
      className="flex flex-col items-center gap-3 px-3 py-12"
      data-message-list-empty="true"
    >
      <Inbox size={40} strokeWidth={1.5} className="text-muted-foreground/40" />
      <div className="text-center">
        <p className="text-sm font-medium text-muted-foreground">
          No messages here yet
        </p>
        <p className="mt-1 text-xs text-muted-foreground/60">
          Messages will appear as they arrive
        </p>
      </div>
    </div>
  )
}
