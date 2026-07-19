/**
 * The platform-services contract (R11): the capabilities pure UI may ask of
 * the host it runs in, without importing `desktop/`, `surfaces/`, or `app/`.
 *
 * Mirrors `lib/command.ts` for input: components consume this context; the
 * composition root (`app/shell/PlatformServicesProvider.tsx`) binds it to the
 * desktop bridge or the web fallbacks. Every function keeps the exact
 * semantics of the module it fronts — no-ops off-desktop stay no-ops here.
 */
import { createContext, useContext } from 'react'

import type { SurfaceDescriptor } from '../../domain/surface/types'
import type { Store } from '../store'

/** Desktop runtime info behind the `get_diagnostics_info` bridge op. */
export interface DiagnosticsInfo {
  appVersion: string
  os: string
  arch: string
  logDirPath: string | null
}

/** A checked, installable desktop update; `prompt` shows the install toast. */
interface AvailableUpdate {
  version: string
  prompt: () => void
}

export interface PlatformServices {
  /** Open a surface in the focused context: route takeover on the web, a
   *  dedicated OS window on desktop. */
  openSurface: (surface: SurfaceDescriptor) => void
  /** Open a surface as a separate window (OS window or web popup). */
  openSurfaceInSeparateWindow: (surface: SurfaceDescriptor) => Promise<void>
  /** Open a URL outside the app (OS browser, or `_blank` with popup guard). */
  openExternalUrl: (url: string) => Promise<void>
  updates: {
    /** `null` when already up to date (always `null` off-desktop). */
    check: () => Promise<AvailableUpdate | null>
  }
  repair: {
    canFactoryReset: () => boolean
    factoryResetAndRestart: () => Promise<void>
    repairLocalDatabaseAndRestart: () => Promise<void>
  }
  /** The client-local "Developer tools" flag (localStorage-synced store). */
  developerTools: Store<boolean>
  diagnostics: {
    /** `null` off-desktop; browser callers fall back to navigator facts. */
    getInfo: () => Promise<DiagnosticsInfo | null>
    revealLogFolder: () => Promise<void>
  }
}

const PlatformServicesContext = createContext<PlatformServices | null>(null)

export const PlatformServicesProvider = PlatformServicesContext.Provider

/** The host capabilities for this document. Throws outside the provider —
 *  a component rendered outside the composition root is a wiring bug. */
export function usePlatformServices(): PlatformServices {
  const services = useContext(PlatformServicesContext)
  if (!services) {
    throw new Error('usePlatformServices: no PlatformServicesProvider mounted')
  }
  return services
}
