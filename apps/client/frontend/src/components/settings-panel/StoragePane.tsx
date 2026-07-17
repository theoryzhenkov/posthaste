/**
 * Storage preferences: how much mail Posthaste caches on this device and what
 * it keeps. Edits `cachePolicy` via the settings PATCH (the same `app.toml`
 * `[cache]` that already existed but had no UI).
 *
 */
import { useState } from 'react'

import type { CachePolicy } from '../../api/types'
import { cn } from '../../lib/utils'
import { SettingsPage, SettingsPageHeader, SettingsSection } from './shared'

const MB = 1024 * 1024
const bytesToMb = (bytes: number): number => Math.round(bytes / MB)
const mbToBytes = (mb: number): number => Math.max(0, Math.round(mb)) * MB

/** A number-of-MB field that commits on blur (one PATCH per edit, not keystroke). */
function CapField({
  label,
  description,
  bytes,
  disabled,
  onCommit,
}: {
  label: string
  description: string
  bytes: number
  disabled: boolean
  onCommit: (bytes: number) => void
}) {
  // Local draft so typing doesn't PATCH per keystroke; re-sync when the
  // committed `bytes` changes (e.g. server clamping) via the adjust-state-
  // during-render pattern rather than an effect.
  const [draft, setDraft] = useState(() => String(bytesToMb(bytes)))
  const [syncedBytes, setSyncedBytes] = useState(bytes)
  if (bytes !== syncedBytes) {
    setSyncedBytes(bytes)
    setDraft(String(bytesToMb(bytes)))
  }

  return (
    <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
      <div className="min-w-0">
        <p className="text-[13px] font-medium text-foreground">{label}</p>
        <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
          {description}
        </p>
      </div>
      <div className="flex items-center gap-2 sm:justify-self-end">
        <input
          type="number"
          min={0}
          inputMode="numeric"
          value={draft}
          disabled={disabled}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={() => {
            const mb = Number(draft)
            if (Number.isFinite(mb) && mb >= 0) {
              onCommit(mbToBytes(mb))
            } else {
              setDraft(String(bytesToMb(bytes)))
            }
          }}
          className="h-8 w-24 rounded-md border border-border bg-background px-2 text-right text-[13px] shadow-none ph-focus-ring"
        />
        <span className="text-[12px] text-muted-foreground">MB</span>
      </div>
    </div>
  )
}

function Toggle({
  label,
  description,
  checked,
  disabled,
  onChange,
}: {
  label: string
  description: string
  checked: boolean
  disabled: boolean
  onChange: (checked: boolean) => void
}) {
  return (
    <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
      <div className="min-w-0">
        <p className="text-[13px] font-medium text-foreground">{label}</p>
        <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
          {description}
        </p>
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={cn(
          'ph-focus-ring relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors sm:justify-self-end',
          checked
            ? 'bg-[var(--brand-coral)]'
            : 'bg-[color-mix(in_oklab,var(--foreground)_22%,transparent)]',
        )}
      >
        <span
          className={cn(
            'inline-block size-4 rounded-full bg-white shadow-sm transition-transform',
            checked ? 'translate-x-4' : 'translate-x-0.5',
          )}
        />
      </button>
    </div>
  )
}

export function StoragePane({
  cachePolicy,
  onChange,
  isPending,
}: {
  cachePolicy: CachePolicy | undefined
  onChange: (policy: CachePolicy) => void
  isPending: boolean
}) {
  if (!cachePolicy) {
    return (
      <SettingsPage>
        <SettingsPageHeader
          title="Storage"
          description="Control how much mail Posthaste caches on this device."
        />
      </SettingsPage>
    )
  }

  const patch = (partial: Partial<CachePolicy>) =>
    onChange({ ...cachePolicy, ...partial })

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Storage"
        description="Control how much mail Posthaste caches on this device and what it keeps. Your mail on the server is never affected."
      />

      <SettingsSection title="Cache size">
        <CapField
          label="Start evicting at"
          description="Once the cache passes this size, Posthaste begins evicting the oldest content."
          bytes={cachePolicy.softCapBytes}
          disabled={isPending}
          onCommit={(bytes) => patch({ softCapBytes: bytes })}
        />
        <CapField
          label="Hard limit"
          description="The cache never grows past this. Re-fetched on demand when needed."
          bytes={cachePolicy.hardCapBytes}
          disabled={isPending}
          onCommit={(bytes) => patch({ hardCapBytes: bytes })}
        />
      </SettingsSection>

      <SettingsSection title="What to cache">
        <Toggle
          label="Message bodies"
          description="Keep rendered message text + HTML for offline reading."
          checked={cachePolicy.cacheBodies}
          disabled={isPending}
          onChange={(value) => patch({ cacheBodies: value })}
        />
        <Toggle
          label="Raw messages"
          description="Keep the original MIME source. Larger; off by default."
          checked={cachePolicy.cacheRawMessages}
          disabled={isPending}
          onChange={(value) => patch({ cacheRawMessages: value })}
        />
        <Toggle
          label="Attachments"
          description="Keep downloaded attachments cached on disk."
          checked={cachePolicy.cacheAttachments}
          disabled={isPending}
          onChange={(value) => patch({ cacheAttachments: value })}
        />
      </SettingsSection>
    </SettingsPage>
  )
}
