/**
 * Ordered instrumentation for the standalone-surface bootstrap + render path.
 *
 * History: a Windows (WebView2) settings-surface window rendered BLACK,
 * UNCLOSABLE, at 0% CPU, with ZERO markers — root-caused to the desktop
 * backend creating the window from a *synchronous* Tauri command, which
 * deadlocks WebView2 controller creation on Windows (the webview never
 * existed, so no JS ran; see `open_surface_window` in
 * apps/desktop/src/desktop_windows.rs). Fixed by making that command async.
 * The markers stay as the watchdog: the surface URL always loads the bundled
 * index.html, so `main_entry` MUST appear once the webview is created — a
 * still-marker-less failure means webview creation itself failed again.
 *
 * Routing: `syncLogger.info` → pino browser `write` → `invoke('log_from_frontend')`
 * — the same frontend→backend path `consoleCapture`/the sync logger already use,
 * so these land in the backend log file the tester can share. INFO level so they
 * survive the production log level (`debug` is dropped in the shipped build).
 * Fire-and-forget: the marker is flushed onto the IPC queue BEFORE the step it
 * precedes, so a subsequent synchronous block can't swallow it.
 *
 * Gated to surface documents only (the main window renders the same bundle) so
 * it stays cheap and un-noisy everywhere the hang does not reproduce.
 */
import { LOG_EVENTS, syncLogger } from './logger'
import { isSurfaceLocation } from '../../domain/surface/location'

/** True when THIS document is a standalone surface window (its own WebView2
 *  renderer) — the only place the hang reproduces. Computed once at load. */
const IS_SURFACE_DOCUMENT =
  typeof window !== 'undefined' &&
  isSurfaceLocation({
    hash: window.location.hash,
    pathname: window.location.pathname,
    search: window.location.search,
  })

const onceSeen = new Set<string>()

function detailSuffix(extra?: Record<string, unknown>): string {
  if (!extra) return ''
  const parts = Object.entries(extra).map(
    ([key, value]) => `${key}=${String(value)}`,
  )
  return parts.length > 0 ? ` ${parts.join(' ')}` : ''
}

/**
 * Emit a surface-bootstrap marker. The step name (and any extras) are folded
 * into the message string so they reach the backend log — the frontend→backend
 * bridge forwards `message` + `event`, not arbitrary fields. No-op outside a
 * surface window.
 */
export function markSurfaceBootstrap(
  step: string,
  extra?: Record<string, unknown>,
): void {
  if (!IS_SURFACE_DOCUMENT) return
  syncLogger.info(
    { event: LOG_EVENTS.surfaceBootstrap, step, ...extra },
    `surface.bootstrap.${step}${detailSuffix(extra)}`,
  )
}

/** Like {@link markSurfaceBootstrap} but fires at most once per `step` — for
 *  render-body call sites that React may re-run, so a re-render loop can't
 *  bury the ordered trace. */
export function markSurfaceBootstrapOnce(
  step: string,
  extra?: Record<string, unknown>,
): void {
  if (!IS_SURFACE_DOCUMENT || onceSeen.has(step)) return
  onceSeen.add(step)
  markSurfaceBootstrap(step, extra)
}
