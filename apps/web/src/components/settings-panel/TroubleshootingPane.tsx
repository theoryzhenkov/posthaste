/**
 * Troubleshooting surface: repair/reset pathways + developer tools.
 *
 * Desktop-reachable (the rail entry is hidden off-Tauri); the repair/reset ops
 * are desktop bridge calls, so this pane is only surfaced where they exist.
 */
import {
  setDeveloperToolsEnabled,
  useDeveloperToolsEnabled,
} from '../../developerTools'
import { cn } from '../../lib/utils'
import { SettingsPage, SettingsPageHeader, SettingsSection } from './shared'
import { TroubleshootingSection } from './TroubleshootingSection'

export function TroubleshootingPane() {
  const developerToolsEnabled = useDeveloperToolsEnabled()

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Troubleshooting"
        description="Repair, reset, and developer tools for when something goes wrong."
      />

      <TroubleshootingSection />

      <SettingsSection title="Developer">
        <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
          <div className="min-w-0">
            <p className="text-[13px] font-medium text-foreground">
              Developer tools
            </p>
            <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
              Enable the in-app web inspector, toggled with ⌘⌥I. Off by default.
            </p>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={developerToolsEnabled}
            aria-label="Developer tools"
            onClick={() => setDeveloperToolsEnabled(!developerToolsEnabled)}
            className={cn(
              'ph-focus-ring relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors sm:justify-self-end',
              developerToolsEnabled
                ? 'bg-[var(--brand-coral)]'
                : 'bg-[color-mix(in_oklab,var(--foreground)_22%,transparent)]',
            )}
          >
            <span
              className={cn(
                'inline-block size-4 rounded-full bg-white shadow-sm transition-transform',
                developerToolsEnabled ? 'translate-x-4' : 'translate-x-0.5',
              )}
            />
          </button>
        </div>
      </SettingsSection>
    </SettingsPage>
  )
}
