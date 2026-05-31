/**
 * {@link ActiveConnectionProvider}: owns the connection-profile store lifecycle.
 * Loads the store on mount, resolves the active profile, re-points the runtime
 * holder (`applyResolvedConnection`) so `api/client.ts` targets the right
 * daemon, and invalidates react-query on a switch.
 *
 * For the default/bundled build this is effectively invisible: the store seeds
 * the embedded profile auto-active, resolution returns the injected connection,
 * and the app renders as today. For the client-only build with no profile, the
 * status is `needs-connection` and the app shows the connect screen instead of
 * firing API calls.
 *
 * The context, types, and `useActiveConnection` hook live in
 * `./connectionContext` so this file only exports a component.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#connection-profiles
 */
import { useQueryClient } from '@tanstack/react-query'
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react'

import {
  ActiveConnectionContext,
  type ActiveConnectionContextValue,
  type ActiveConnectionStatus,
  type AddProfileInput,
} from './connectionContext'
import { resolveActiveConnection } from './resolve'
import { applyResolvedConnection } from './runtime'
import { clientStore, defaultConnectionsFile } from './store'
import { type ConnectionProfile, type ConnectionsFile } from './types'

function makeProfileId(): string {
  if (
    typeof crypto !== 'undefined' &&
    typeof crypto.randomUUID === 'function'
  ) {
    return crypto.randomUUID()
  }
  return `profile-${Date.now()}-${Math.random().toString(36).slice(2)}`
}

export function ActiveConnectionProvider({
  children,
}: {
  children: ReactNode
}): ReactNode {
  const queryClient = useQueryClient()
  const [file, setFile] = useState<ConnectionsFile>(() =>
    defaultConnectionsFile(),
  )
  const [status, setStatus] = useState<ActiveConnectionStatus>('loading')
  const [reason, setReason] = useState<string | null>(null)
  // Guard against applying a stale async resolution after a newer switch.
  const generation = useRef(0)

  const supportsSecureTokens = clientStore().supportsSecureTokens

  const resolveAndApply = useCallback(async () => {
    const gen = ++generation.current
    const resolution = await resolveActiveConnection()
    if (gen !== generation.current) {
      return
    }
    if (resolution.status === 'connected') {
      applyResolvedConnection(resolution.connection, resolution.profileId)
      setStatus('connected')
      setReason(null)
    } else {
      setStatus('needs-connection')
      setReason(resolution.reason)
    }
  }, [])

  // Initial load: read the store, then resolve + apply the active connection.
  useEffect(() => {
    let cancelled = false
    void (async () => {
      const loaded = await clientStore().loadConnections()
      if (cancelled) {
        return
      }
      setFile(loaded)
      await resolveAndApply()
    })()
    return () => {
      cancelled = true
    }
  }, [resolveAndApply])

  const persist = useCallback(async (next: ConnectionsFile) => {
    setFile(next)
    await clientStore().saveConnections(next)
  }, [])

  const addProfile = useCallback(
    async (input: AddProfileInput) => {
      const id = makeProfileId()
      const profile: ConnectionProfile = {
        id,
        name: input.name,
        mode: input.mode,
        baseUrl: input.baseUrl,
        hostHeader: input.hostHeader,
        tokenRef: input.mode === 'remote' ? id : undefined,
      }
      if (input.mode === 'remote' && input.token) {
        await clientStore().setToken(id, input.token)
      }
      await persist({
        version: 1,
        activeProfileId: id,
        profiles: [...file.profiles, profile],
      })
      await resolveAndApply()
    },
    [file.profiles, persist, resolveAndApply],
  )

  const selectProfile = useCallback(
    async (id: string) => {
      if (!file.profiles.some((profile) => profile.id === id)) {
        return
      }
      await persist({ ...file, activeProfileId: id })
      await resolveAndApply()
      // Drop cached data from the previous daemon and refetch against the new one.
      await queryClient.invalidateQueries()
    },
    [file, persist, queryClient, resolveAndApply],
  )

  const removeProfile = useCallback(
    async (id: string) => {
      const remaining = file.profiles.filter((profile) => profile.id !== id)
      const nextActiveId =
        file.activeProfileId === id
          ? (remaining[0]?.id ?? null)
          : file.activeProfileId
      await clientStore().deleteToken(id)
      await persist({
        version: 1,
        activeProfileId: nextActiveId,
        profiles: remaining,
      })
      await resolveAndApply()
      await queryClient.invalidateQueries()
    },
    [file, persist, queryClient, resolveAndApply],
  )

  const value = useMemo<ActiveConnectionContextValue>(
    () => ({
      status,
      reason,
      profiles: file.profiles,
      activeProfileId: file.activeProfileId,
      addProfile,
      selectProfile,
      removeProfile,
      refresh: resolveAndApply,
      supportsSecureTokens,
    }),
    [
      status,
      reason,
      file.profiles,
      file.activeProfileId,
      addProfile,
      selectProfile,
      removeProfile,
      resolveAndApply,
      supportsSecureTokens,
    ],
  )

  return (
    <ActiveConnectionContext.Provider value={value}>
      {children}
    </ActiveConnectionContext.Provider>
  )
}
