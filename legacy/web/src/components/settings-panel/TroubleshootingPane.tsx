/**
 * Troubleshooting surface: repair/reset pathways + developer tools.
 *
 * Desktop-reachable (the rail entry is hidden off-Tauri); the repair/reset ops
 * are desktop bridge calls, so this pane is only surfaced where they exist.
 */
import { Clipboard, FolderOpen } from 'lucide-react'
import {
  setDeveloperToolsEnabled,
  useDeveloperToolsEnabled,
} from '../../developerTools'
import { useDiagnosticsBundle } from '../../hooks/useDiagnosticsBundle'
import { cn } from '../../lib/utils'
import { Button } from '../ui/button'
import { SettingsAdvanced } from './SettingsAdvanced'
import { SettingsPage, SettingsPageHeader, SettingsSection } from './shared'
import { TroubleshootingSection } from './TroubleshootingSection'

function DiagnosticsSection() {
  const { isDesktop, logDirPath, copyDiagnostics, revealLogFolder } =
    useDiagnosticsBundle()
  return (
    <SettingsSection title="Diagnostics">
      <div className="grid gap-3 sm:grid-cols-[1fr_280px] sm:items-center">
        <div className="min-w-0">
          <p className="text-[13px] font-medium text-foreground">
            Copy diagnostics
          </p>
          <p className="mt-1 text-[12px] leading-5 text-muted-foreground">
            Copy a sanitized summary of the app version, platform, and account
            status to paste into a bug report. No message bodies, passwords, or
            email addresses are included.
          </p>
        </div>
        <div className="flex flex-wrap gap-2 sm:justify-self-end">
          {isDesktop ? (
            <Button
              type="button"
              variant="outline"
              onClick={() => void revealLogFolder()}
              className="h-8 gap-2 border-border bg-background text-[13px] shadow-none"
            >
              <FolderOpen size={14} />
              Reveal logs
            </Button>
          ) : null}
          <Button
            type="button"
            variant="outline"
            onClick={() => void copyDiagnostics()}
            className="h-8 gap-2 border-border bg-background text-[13px] shadow-none"
          >
            <Clipboard size={14} />
            Copy
          </Button>
        </div>
      </div>
      {logDirPath ? (
        <p className="break-all text-[11px] leading-5 text-muted-foreground">
          Logs: <code className="font-mono">{logDirPath}</code>
        </p>
      ) : null}
    </SettingsSection>
  )
}

export function TroubleshootingPane() {
  const developerToolsEnabled = useDeveloperToolsEnabled()

  return (
    <SettingsPage>
      <SettingsPageHeader
        title="Troubleshooting"
        description="Repair, reset, and developer tools for when something goes wrong."
      />

      <TroubleshootingSection />

      <DiagnosticsSection />

      <SettingsAdvanced label="Developer">
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
      </SettingsAdvanced>
    </SettingsPage>
  )
}
