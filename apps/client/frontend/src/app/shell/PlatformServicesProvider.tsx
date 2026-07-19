/**
 * Binds the platform-services contract (`lib/platform/services.ts`) to the
 * real capabilities: surface navigation composed in `app/host/navigation.ts`,
 * and the desktop bridge modules (updates, repair, devtools, diagnostics).
 * Every binding is a plain passthrough — behavior lives in the bound modules.
 */
import { useMemo, type ReactNode } from 'react'

import { developerToolsStore } from '@/desktop/devtools'
import { getDiagnosticsInfo, revealLogFolder } from '@/desktop/diagnostics/info'
import {
  canFactoryReset,
  factoryResetAndRestart,
  repairLocalDatabaseAndRestart,
} from '@/desktop/repair/repair'
import { openExternalUrl } from '@/desktop/runtime'
import {
  checkForDesktopUpdate,
  promptDesktopUpdate,
} from '@/desktop/updates/updates'
import {
  PlatformServicesProvider as ContractProvider,
  type PlatformServices,
} from '@/lib/platform/services'
import {
  openFocusedSurface,
  openSurfaceInSeparateWindow,
} from '../host/navigation'

export function PlatformServicesProvider({
  children,
}: {
  children: ReactNode
}) {
  const services = useMemo<PlatformServices>(
    () => ({
      openSurface: openFocusedSurface,
      openSurfaceInSeparateWindow,
      openExternalUrl,
      updates: {
        check: async () => {
          const update = await checkForDesktopUpdate()
          return update
            ? {
                version: update.version,
                prompt: () => promptDesktopUpdate(update),
              }
            : null
        },
      },
      repair: {
        canFactoryReset,
        factoryResetAndRestart,
        repairLocalDatabaseAndRestart,
      },
      developerTools: developerToolsStore,
      diagnostics: {
        getInfo: getDiagnosticsInfo,
        revealLogFolder,
      },
    }),
    [],
  )
  return <ContractProvider value={services}>{children}</ContractProvider>
}
