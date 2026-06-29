/**
 * Gather + copy a sanitized diagnostics bundle for beta support.
 *
 * Desktop-only fields (version, OS, arch, log directory path) come from the
 * `get_diagnostics_info` Tauri command; in the browser/dev build they fall back
 * to the release channel + `navigator.platform`. Account data is read from the
 * canonical accounts query cache (fed by {@link useAccountsView}); only
 * structural fields survive into the bundle (see {@link formatDiagnosticsBundle}).
 *
 * @see docs/eph/REPORT-L2-public-beta-readiness-audit.md (diagnostics/support)
 */
import { useCallback, useEffect, useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'
import { toast } from 'sonner'

import { isTauriRuntime } from '@/desktop'
import { formatDiagnosticsBundle } from '@/diagnostics'
import { LOG_EVENTS } from '@/logEvents'
import { syncLogger } from '@/logger'
import { releaseChannel } from '@/runtime/releaseChannel'
import { runtimeViews } from '@/runtime/views'
import { queryKeys } from '@/queryKeys'

/** Runtime info returned by the `get_diagnostics_info` desktop bridge op. */
interface DiagnosticsInfo {
  appVersion: string
  os: string
  arch: string
  logDirPath: string | null
}

export interface UseDiagnosticsBundleResult {
  isDesktop: boolean
  logDirPath: string | null
  copyDiagnostics: () => Promise<void>
  revealLogFolder: () => Promise<void>
}

/**
 * Best-effort clipboard write with a fallback for non-secure contexts (older
 * WebKitGTK). Returns `true` on success.
 */
async function copyText(text: string): Promise<boolean> {
  if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(text)
      return true
    } catch {
      // Fall through to the legacy path.
    }
  }
  try {
    const textarea = document.createElement('textarea')
    textarea.value = text
    textarea.setAttribute('readonly', '')
    textarea.style.position = 'fixed'
    textarea.style.opacity = '0'
    document.body.appendChild(textarea)
    textarea.select()
    const ok = document.execCommand('copy')
    document.body.removeChild(textarea)
    return ok
  } catch {
    return false
  }
}

export function useDiagnosticsBundle(): UseDiagnosticsBundleResult {
  const isDesktop = isTauriRuntime()
  const [info, setInfo] = useState<DiagnosticsInfo | null>(null)

  // Read the canonical accounts cache (fed by `useAccountsView` at the app
  // root). `enabled: false` means we never fetch from here — we only observe.
  const { data: accounts = [] } = useQuery({
    queryKey: queryKeys.accounts,
    queryFn: runtimeViews.accounts.list,
    enabled: false,
  })

  useEffect(() => {
    if (!isDesktop) {
      return
    }
    let cancelled = false
    invoke<DiagnosticsInfo>('get_diagnostics_info')
      .then((result) => {
        if (!cancelled) {
          setInfo(result)
        }
      })
      .catch((error) => {
        syncLogger.warn(
          { event: LOG_EVENTS.diagnosticsCopyFailed, error },
          'could not load diagnostics info',
        )
      })
    return () => {
      cancelled = true
    }
  }, [isDesktop])

  const copyDiagnostics = useCallback(async () => {
    const bundle = formatDiagnosticsBundle({
      appVersion: info?.appVersion ?? releaseChannel,
      releaseChannel,
      os: info?.os ?? (typeof navigator !== 'undefined' ? navigator.platform : 'unknown'),
      arch: info?.arch ?? '',
      logDirPath: info?.logDirPath ?? null,
      accounts,
      generatedAt: new Date(),
    })
    const ok = await copyText(bundle)
    if (ok) {
      toast.success('Diagnostics copied to clipboard.')
      syncLogger.info(
        { event: LOG_EVENTS.diagnosticsCopied },
        'diagnostics bundle copied',
      )
    } else {
      syncLogger.warn(
        { event: LOG_EVENTS.diagnosticsCopyFailed },
        'diagnostics clipboard write failed',
      )
      toast.error('Could not copy diagnostics. Please try again.')
    }
  }, [accounts, info])

  const revealLogFolder = useCallback(async () => {
    if (!isDesktop) {
      return
    }
    try {
      await invoke('reveal_log_folder')
    } catch (error) {
      syncLogger.warn(
        { event: LOG_EVENTS.diagnosticsCopyFailed, error },
        'could not reveal log folder',
      )
      toast.error('Could not open the log folder.')
    }
  }, [isDesktop])

  return {
    isDesktop,
    logDirPath: info?.logDirPath ?? null,
    copyDiagnostics,
    revealLogFolder,
  }
}
