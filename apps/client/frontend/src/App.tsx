/**
 * Root application component: QueryClientProvider, stream-driven invalidation,
 * three-column layout, and focused surfaces.
 */
import { QueryClientProvider } from '@tanstack/react-query'
import { useEffect } from 'react'
import { Toaster } from 'sonner'

// Bootstrap: side-effect import populates the action registry at app init, so
// it is filled before any surface (context menu, etc.) resolves from it.
import './actions'
import { ConnectionBanner } from './app/ConnectionBanner'
import { MailClient } from './app/MailClient'
import { Z } from './layering'
import { queryClient, useStreamInvalidation } from './data'
import { ErrorBoundary } from './components/ErrorBoundary'
import { FocusedSurfaceDocument } from './components/FocusedSurface'
import { InvalidSurfaceDocument } from './components/InvalidSurface'
import { DesignThemeProvider } from './components/ThemeProvider'
import { isMainDesktopWindow, isTauriRuntime, toggleDevtools } from './desktop'
import { isDeveloperToolsEnabled } from './developerTools'
import { useNewMailNotifications } from './notifications/newMailArrivals'
import { DockBadge } from './hooks/DockBadge'
import { useDesktopUpdates } from './hooks/useDesktopUpdates'
import { AppearanceSettingsSync } from './hooks/useAppearanceSettingsSync'
import { useSurfaceRouteState } from './hooks/useSurfaceRouting'
import {
  markSurfaceBootstrap,
  markSurfaceBootstrapOnce,
} from './surfaceBootstrapLog'

/** The ONE liveness policy: a generation advance on the event stream
 * invalidates every query react-query holds (debounced). */
function StreamInvalidationBridge() {
  useStreamInvalidation()
  return null
}

/** New-mail OS banners: `message.updated` payloads prompt the arrival gate. */
function NewMailNotificationsBridge() {
  useNewMailNotifications()
  return null
}

function renderAppRootError(error: Error) {
  return (
    <div className="fixed inset-0 flex flex-col items-center justify-center gap-3 bg-background p-6 text-center">
      <p className="text-sm font-medium text-foreground">
        The app hit an unexpected error
      </p>
      <p className="max-w-md text-xs break-words text-muted-foreground">
        {error.message}
      </p>
      <button
        type="button"
        className="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-muted"
        onClick={() => window.location.reload()}
      >
        Reload
      </button>
    </div>
  )
}

export default function App() {
  const routeState = useSurfaceRouteState()
  const routeSurface = routeState.kind === 'valid' ? routeState.surface : null
  const invalidSurfaceRoute =
    routeState.kind === 'invalid' ? routeState.route : null
  const isStandaloneSurface =
    isTauriRuntime() && routeState.kind !== 'none' && !isMainDesktopWindow()

  markSurfaceBootstrapOnce('app_render', {
    isStandaloneSurface,
    routeKind: routeState.kind,
    surfaceKind: routeSurface?.kind ?? null,
  })
  useEffect(() => {
    markSurfaceBootstrap('app_mounted', { isStandaloneSurface })
  }, [isStandaloneSurface])

  useDeveloperToolsShortcut()
  useDesktopUpdates()

  return (
    <QueryClientProvider client={queryClient}>
      <DesignThemeProvider writeThrough>
        <AppearanceSettingsSync />
        {/* Liveness rides the facade's event stream in the MAIN window; a
            standalone surface window keeps its queries mount-fetched only. */}
        {!isStandaloneSurface && <StreamInvalidationBridge key="mail" />}
        {/* New-mail banners ride the same stream — main window only, so a
            secondary surface window never double-notifies. */}
        {!isStandaloneSurface && <NewMailNotificationsBridge />}
        {!isStandaloneSurface && <ConnectionBanner />}
        {/* App-wide unread badge — main window only; a standalone surface
            window must not drive the shared Dock counter to 0. */}
        {!isStandaloneSurface && <DockBadge />}
        <ErrorBoundary label="app-root" fallback={renderAppRootError}>
          {isStandaloneSurface && routeSurface ? (
            <FocusedSurfaceDocument surface={routeSurface} />
          ) : isStandaloneSurface && invalidSurfaceRoute ? (
            <InvalidSurfaceDocument route={invalidSurfaceRoute} />
          ) : (
            <MailClient
              invalidSurfaceRoute={invalidSurfaceRoute}
              routeSurface={routeSurface}
            />
          )}
        </ErrorBoundary>
        <Toaster
          position="bottom-center"
          // TOAST tier: above windows/overlay/modals, below tooltips. Overrides
          // sonner's very-high default so it sits inside the app's scale.
          style={{ zIndex: Z.TOAST }}
          toastOptions={{ className: 'font-sans text-sm' }}
        />
      </DesignThemeProvider>
    </QueryClientProvider>
  )
}

function useDeveloperToolsShortcut() {
  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (
        (event.metaKey || event.ctrlKey) &&
        event.altKey &&
        event.code === 'KeyI' &&
        isDeveloperToolsEnabled()
      ) {
        event.preventDefault()
        void toggleDevtools()
      }
    }
    window.addEventListener('keydown', handleKeyDown)
    return () => window.removeEventListener('keydown', handleKeyDown)
  }, [])
}
