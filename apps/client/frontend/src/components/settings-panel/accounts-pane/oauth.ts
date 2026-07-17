// The client half of the OAuth account flow. `oauthStart` is a read that
// mints an authorization descriptor; the browser visits its URL and the
// provider redirects back to the app with `?code&state`, which
// `useOauthCallbackCapture` turns into the `completeOauth` command (the
// backend exchanges the code, stores the token set, and creates the
// account — visible in the next accounts answer).

import { useEffect, useRef, useState } from 'react'

import { fetchQuery, useCommands, useMailClient } from '@/data'
import type { OauthStartQuery, OauthStartResult } from '@/gen'

/**
 * Where the provider sends the browser back: the running app itself. The
 * same URI must be registered with the provider's OAuth app registration.
 */
export function oauthRedirectUri(): string {
  return `${window.location.origin}${window.location.pathname}`
}

export function useStartOauth() {
  const client = useMailClient()
  return (query: OauthStartQuery) =>
    fetchQuery<OauthStartResult>(client, { oauthStart: query })
}

export type OauthCallbackState =
  | { kind: 'idle' }
  | { kind: 'completing' }
  | { kind: 'done' }
  | { kind: 'error'; message: string }

/**
 * Detects a provider redirect landing (`?code&state` on the current URL),
 * posts `completeOauth`, and strips the parameters from the address bar.
 * Mount once wherever the redirect can land.
 */
export function useOauthCallbackCapture(): OauthCallbackState {
  const commands = useCommands()
  const [state, setState] = useState<OauthCallbackState>({ kind: 'idle' })
  const startedRef = useRef(false)

  useEffect(() => {
    if (startedRef.current) return
    const params = new URLSearchParams(window.location.search)
    const code = params.get('code')
    const oauthState = params.get('state')
    if (!code || !oauthState) return
    startedRef.current = true

    params.delete('code')
    params.delete('state')
    params.delete('scope')
    params.delete('session_state')
    const query = params.toString()
    const cleaned =
      window.location.pathname + (query ? `?${query}` : '') + window.location.hash
    window.history.replaceState(null, '', cleaned)

    setState({ kind: 'completing' })
    commands
      .run({ completeOauth: { state: oauthState, code } })
      .then(() => setState({ kind: 'done' }))
      .catch((error: unknown) =>
        setState({
          kind: 'error',
          message:
            error instanceof Error
              ? error.message
              : 'Completing the provider authorization failed.',
        }),
      )
  }, [commands])

  return state
}
