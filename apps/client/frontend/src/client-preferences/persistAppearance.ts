// Persisting the renderer's appearance preferences into the app settings
// document. The settings document is stored whole, so the write is a
// read-modify-write: fetch the current `appSettings` answer fresh, fold the
// appearance in, and post one `updateSettings` command (whose acceptance
// invalidates every mounted query).

import type { QueryClient } from '@tanstack/react-query'
import type { MailClient } from '@/client'
import type { AppSettingsResult } from '@/gen'
import { fetchQuery } from '@/data/queries'
import { runCommand } from '@/data/commands'
import type { DesignThemePreferences } from '@/themeSettings'

import { designToWireAppearance } from './wireMapping'

export async function persistAppearance(
  client: MailClient,
  queryClient: QueryClient,
  prefs: DesignThemePreferences,
): Promise<void> {
  const { settings } = await fetchQuery<AppSettingsResult>(client, {
    appSettings: {},
  })
  await runCommand(client, queryClient, {
    updateSettings: {
      settings: { ...settings, appearance: designToWireAppearance(prefs) },
      forceBackfill: false,
    },
  })
}
