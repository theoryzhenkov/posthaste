/**
 * Notification preferences: new-mail alerts and sounds. Edits `notifications`
 * on the settings document via the `updateSettings` command. OS-level
 * delivery permission stays device-local (not a config concern): it is
 * requested LAZILY when the user turns "New mail" on — never at boot — and a
 * denial is surfaced inline so the user knows why banners stay silent.
 */
import { useState } from 'react'

import type { Notifications } from '../../../data/transport/api/index'
import { cn } from '../../../lib/design/cn'
import {
  requestOsNotificationPermission,
  type OsNotificationPermission,
} from '../../../data/notifications/osNotifier'
import { SettingsPage, SettingsPageHeader, SettingsSection } from '../panel/shared'

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

export function NotificationsPane({
  notifications,
  onChange,
  isPending,
}: {
  notifications: Notifications | null | undefined
  onChange: (notifications: Notifications) => void
  isPending: boolean
}) {
  // Absent fields fall back to the effective default (on) until the user opts out.
  const newMail = notifications?.newMail ?? true
  const sound = notifications?.sound ?? true
  const patch = (partial: Partial<Notifications>) =>
    onChange({ newMail, sound, ...partial })

  // OS delivery permission, requested lazily on enable (never at boot). Known
  // only after the user has toggled here; `null` = not asked this session.
  const [osPermission, setOsPermission] =
    useState<OsNotificationPermission | null>(null)

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Notifications"
        description="Alerts and sounds for new mail. OS-level notification permission is managed by your system."
      />

      <SettingsSection title="Alerts">
        <Toggle
          label="New mail"
          description="Show an alert when new mail arrives."
          checked={newMail}
          disabled={isPending}
          onChange={(value) => {
            patch({ newMail: value })
            if (value) {
              void requestOsNotificationPermission().then(setOsPermission)
            }
          }}
        />
        {newMail && osPermission === 'denied' && (
          <p className="text-[12px] leading-5 text-[var(--brand-coral)]">
            Notifications are blocked at the system level. Allow notifications
            for Posthaste in your OS settings to see new-mail alerts.
          </p>
        )}
        <Toggle
          label="Sounds"
          description="Play a sound for new mail."
          checked={sound}
          disabled={isPending}
          onChange={(value) => patch({ sound: value })}
        />
      </SettingsSection>
    </SettingsPage>
  )
}
