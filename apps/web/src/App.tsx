/**
 * Root application component: QueryClientProvider, toolbar, three-column layout,
 * and focused surfaces.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L0-ui#navigation-model
 */
import { QueryClientProvider } from '@tanstack/react-query'
import { useEffect, type ReactNode } from 'react'
import { Toaster } from 'sonner'

import { MailClient } from './app/MailClient'
import { queryClient } from './app/queryClient'
import { ErrorBoundary } from './components/ErrorBoundary'
import { FocusedSurfaceDocument } from './components/FocusedSurface'
import { InvalidSurfaceDocument } from './components/InvalidSurface'
import { DesignThemeProvider } from './components/ThemeProvider'
import { ConnectionScreen } from './connection/ConnectionScreen'
import { useActiveConnection } from './connection/connectionContext'
import { ActiveConnectionProvider } from './connection/useActiveConnection'
import { isMainDesktopWindow, isTauriRuntime, toggleDevtools } from './desktop'
import { isDeveloperToolsEnabled } from './developerTools'
import { useAccountsView } from './hooks/useAccountsView'
import { useDaemonEvents } from './hooks/useDaemonEvents'
import { useDesktopUpdates } from './hooks/useDesktopUpdates'
import { useReplicaDatabaseReloadPrompt } from './hooks/useReplicaDatabaseReloadPrompt'
import { AppearanceSettingsSync } from './hooks/useAppearanceSettingsSync'
import { useSurfaceRouteState } from './hooks/useSurfaceRouting'

function DaemonEventBridge() {
  useDaemonEvents()
  useAccountsView()
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

/**
 * Gate the app behind a resolvable connection. The active connection is seeded
 * synchronously to the embedded default at module load, so the bundled build
 * renders mail immediately (status `loading` → `connected` with no flash). Only
 * a true `needs-connection` (client-only build with no profile, or an
 * unreachable local/remote daemon) shows the connect screen.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#build-modes
 */
function ConnectionGate({ children }: { children: ReactNode }) {
  const { status } = useActiveConnection()
  if (status === 'needs-connection') {
    return <ConnectionScreen />
  }
  return children
}

export default function App() {
  const routeState = useSurfaceRouteState()
  const routeSurface = routeState.kind === 'valid' ? routeState.surface : null
  const invalidSurfaceRoute =
    routeState.kind === 'invalid' ? routeState.route : null
  const isStandaloneSurface =
    isTauriRuntime() && routeState.kind !== 'none' && !isMainDesktopWindow()

  useDeveloperToolsShortcut()
  useDesktopUpdates()
  useReplicaDatabaseReloadPrompt()

  return (
    <QueryClientProvider client={queryClient}>
      <DesignThemeProvider writeThrough>
        <AppearanceSettingsSync />
        <ActiveConnectionProvider>
          <DaemonEventBridge
            key={isStandaloneSurface ? 'standalone' : 'mail'}
          />
          <ErrorBoundary label="app-root" fallback={renderAppRootError}>
            <ConnectionGate>
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
            </ConnectionGate>
          </ErrorBoundary>
          <Toaster
            position="bottom-center"
            toastOptions={{ className: 'font-sans text-sm' }}
          />
        </ActiveConnectionProvider>
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
