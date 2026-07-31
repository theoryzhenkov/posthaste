/**
 * Root application component: this window's live mirror, three-column layout,
 * and focused surfaces.
 */
import { useEffect } from 'react'
import { Toaster } from 'sonner'

import { CommandDispatcher } from '../commands/index'
import { ConnectionBanner } from './shell/ConnectionBanner'
import { MailClient } from './mail/MailClient'
import { Z } from '@/lib/design/layering'
import { MirrorProvider } from '../data/index'
import { ErrorBoundary } from './shell/ErrorBoundary'
import { FocusedSurfaceDocument } from './host/FocusedSurface'
import { InvalidSurfaceDocument } from './host/InvalidSurface'
import { DesignThemeProvider } from './shell/ThemeProvider'
import { PlatformServicesProvider } from './shell/PlatformServicesProvider'
import { toggleDevtools } from '../desktop/runtime'
import { isMainDesktopWindow, isTauriRuntime } from '@/lib/platform/runtime'
import { useOwnsSharedOsSurfaces } from '@/lib/platform/sharedOsSurfaces'
import { isDeveloperToolsEnabled } from '../desktop/devtools'
import type { CommandScope } from '../lib/command'
import { useNewMailNotifications } from '../data/notifications/newMailArrivals'
import { DockBadge } from '../desktop/dock/DockBadge'
import { useDesktopUpdates } from '../desktop/updates/useDesktopUpdates'
import { AppearanceSettingsSync } from '../data/preferences/useAppearanceSettingsSync'
import { useSurfaceRouteState } from '../surfaces/useSurfaceRouting'
import {
  markSurfaceBootstrap,
  markSurfaceBootstrapOnce,
} from '@/lib/log/surfaceBootstrap'

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
  // Which DOCUMENT this window renders is a question about the window; whether
  // it may drive the Dock badge and the OS banners is a question about the
  // process. Separate concepts, separately answered.
  const ownsSharedOsSurfaces = useOwnsSharedOsSurfaces()

  markSurfaceBootstrapOnce('app_render', {
    isStandaloneSurface,
    routeKind: routeState.kind,
    surfaceKind: routeSurface?.kind ?? null,
  })
  useEffect(() => {
    markSurfaceBootstrap('app_mounted', { isStandaloneSurface })
  }, [isStandaloneSurface])

  useDesktopUpdates()

  // Every window gets a live mirror: MirrorProvider creates this window's
  // QueryClient and subscribes it to the stream in one act, so there is no
  // "and remember to mount the bridge" step left to forget.
  return (
    <MirrorProvider>
      <PlatformServicesProvider>
      <CommandDispatcher scope={APP_COMMAND_SCOPE}>
      <DesignThemeProvider writeThrough>
        <AppearanceSettingsSync />
        <ConnectionBanner />
        {/* Process-wide OS effects — whichever window holds the claim, and
            only that one: the Dock counter is app-wide rather than
            per-window, and two windows on the arrival gate post two banners.
            The claim survives its holder closing (sharedOsSurfaces.ts). */}
        {ownsSharedOsSurfaces && (
          <>
            <NewMailNotificationsBridge />
            <DockBadge />
          </>
        )}
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
      </CommandDispatcher>
      </PlatformServicesProvider>
    </MirrorProvider>
  )
}

/** The root command scope: binds the desktop devtools capability, so
 *  `app.toggle-devtools` (⌘⌥I) resolves in every window — formerly App's own
 *  window listener (the charter's first R4 migration). */
const APP_COMMAND_SCOPE: CommandScope = {
  owner: 'mail',
  services: {
    desktop: {
      isDeveloperToolsEnabled,
      toggleDevtools,
    },
  },
}
