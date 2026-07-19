/**
 * Desktop bridge ops behind the settings Troubleshooting pane's diagnostics
 * section: runtime info for the bundle and the "reveal logs" affordance. Both
 * are graceful no-ops off-desktop; the pane's formatting/clipboard logic lives
 * with the pane (`components/settings/panes/troubleshooting/`).
 */
import { invoke } from '@tauri-apps/api/core'

import type { DiagnosticsInfo } from '../../lib/platform/services'
import { isTauriRuntime } from '../../lib/platform/runtime'

/** Fetch desktop runtime info; `null` in the browser build. Throws when the
 *  bridge op itself fails — callers log and fall back. */
export async function getDiagnosticsInfo(): Promise<DiagnosticsInfo | null> {
  if (!isTauriRuntime()) {
    return null
  }
  return invoke<DiagnosticsInfo>('get_diagnostics_info')
}

export async function revealLogFolder(): Promise<void> {
  if (!isTauriRuntime()) {
    return
  }
  await invoke('reveal_log_folder')
}
