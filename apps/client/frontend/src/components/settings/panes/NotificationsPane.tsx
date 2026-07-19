/**
 * Notification preferences: new-mail alerts and sounds. Edits `notifications`
 * on the settings document via the `updateSettings` command. OS-level
 * delivery permission stays device-local (not a config concern): it is
 * requested LAZILY when the user turns "New mail" on — never at boot — and a
 * denial is surfaced inline so the user knows why banners stay silent.
 */
import { useState } from 'react'

import type { Notifications } from '../../../data/transport/api/index'
import {
  requestOsNotificationPermission,
  type OsNotificationPermission,
} from '../../../data/notifications/osNotifier'
import { SettingsPage, SettingsPageHeader, SettingsSection, SettingsToggle } from '../panel/shared'


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
        <SettingsToggle
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
        <SettingsToggle
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
