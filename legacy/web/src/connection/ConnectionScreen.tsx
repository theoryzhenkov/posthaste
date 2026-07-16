/**
 * Minimal connect screen, shown when the active connection resolves to a
 * `needs-connection` state (e.g. the client-only build with no profile yet, or
 * a local-daemon profile whose daemon is not running). For the bundled build
 * this never renders — the embedded profile is auto-active.
 *
 * Intentionally minimal/functional: add a `local-daemon` or `remote` profile,
 * or retry resolution. Full polish is a follow-up.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#build-modes
 */
import { useState } from 'react'

import { Button } from '../components/ui/button'
import { Input } from '../components/ui/input'
import { isTauriRuntime } from '../desktop'
import { useActiveConnection, type AddProfileInput } from './connectionContext'
import { type ConnectionMode } from './types'

export function ConnectionScreen(): React.ReactNode {
  const { reason, addProfile, refresh, supportsSecureTokens } =
    useActiveConnection()
  const [mode, setMode] = useState<ConnectionMode>(
    isTauriRuntime() ? 'local-daemon' : 'remote',
  )
  const [name, setName] = useState('')
  const [baseUrl, setBaseUrl] = useState('')
  const [hostHeader, setHostHeader] = useState('')
  const [token, setToken] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    setBusy(true)
    setError(null)
    try {
      const input: AddProfileInput = {
        name:
          name.trim() || (mode === 'remote' ? 'Remote daemon' : 'Local daemon'),
        mode,
        baseUrl: mode === 'remote' ? baseUrl.trim() : undefined,
        hostHeader:
          mode === 'remote' && hostHeader.trim()
            ? hostHeader.trim()
            : undefined,
        token: mode === 'remote' && token ? token : undefined,
      }
      await addProfile(input)
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background p-6">
      <form
        onSubmit={submit}
        className="w-full max-w-md space-y-4 rounded-lg border border-border bg-card p-6 shadow-sm"
      >
        <div className="space-y-1">
          <h1 className="text-lg font-semibold">Connect to a daemon</h1>
          <p className="text-sm text-muted-foreground">
            {reason ?? 'Choose a Posthaste daemon to connect to.'}
          </p>
        </div>

        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor="connection-mode">
            Connection type
          </label>
          <select
            id="connection-mode"
            className="h-9 w-full rounded-md border border-input bg-transparent px-3 text-sm"
            value={mode}
            onChange={(event) => setMode(event.target.value as ConnectionMode)}
          >
            {isTauriRuntime() && (
              <option value="local-daemon">Local daemon (this computer)</option>
            )}
            <option value="remote">Remote daemon (URL)</option>
          </select>
        </div>

        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor="connection-name">
            Name
          </label>
          <Input
            id="connection-name"
            value={name}
            placeholder={mode === 'remote' ? 'Remote daemon' : 'Local daemon'}
            onChange={(event) => setName(event.target.value)}
          />
        </div>

        {mode === 'remote' && (
          <>
            <div className="space-y-2">
              <label className="text-sm font-medium" htmlFor="connection-url">
                Base URL
              </label>
              <Input
                id="connection-url"
                value={baseUrl}
                placeholder="http://daemon.tailnet:3001/v1"
                onChange={(event) => setBaseUrl(event.target.value)}
                required
              />
            </div>
            <div className="space-y-2">
              <label className="text-sm font-medium" htmlFor="connection-host">
                Host header (optional)
              </label>
              <Input
                id="connection-host"
                value={hostHeader}
                placeholder="daemon.tailnet"
                onChange={(event) => setHostHeader(event.target.value)}
              />
            </div>
            <div className="space-y-2">
              <label className="text-sm font-medium" htmlFor="connection-token">
                Token
              </label>
              <Input
                id="connection-token"
                type="password"
                value={token}
                onChange={(event) => setToken(event.target.value)}
              />
              {!supportsSecureTokens && (
                <p className="text-xs text-muted-foreground">
                  This build has no secure token store; remote tokens require
                  the desktop app.
                </p>
              )}
            </div>
          </>
        )}

        {error && <p className="text-sm text-destructive">{error}</p>}

        <div className="flex gap-2">
          <Button type="submit" disabled={busy}>
            {busy ? 'Connecting…' : 'Connect'}
          </Button>
          <Button
            type="button"
            variant="outline"
            disabled={busy}
            onClick={() => void refresh()}
          >
            Retry
          </Button>
        </div>
      </form>
    </div>
  )
}
