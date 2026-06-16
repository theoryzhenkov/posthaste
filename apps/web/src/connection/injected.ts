/**
 * Readers for the embedded-server injection (`__POSTHASTE_PORT__` /
 * `__POSTHASTE_TOKEN__`). Present in the bundled desktop build and absent in the
 * client-only build / browser dev. Centralized here so both the legacy default
 * path and the `embedded` profile resolution read the globals identically.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#connection-profiles
 */

function normalizeApiBaseUrl(baseUrl: string): string {
  return baseUrl.replace(/\/+$/, '')
}

export type InjectedRuntimeMode = 'loopback' | 'native'

interface InjectedRuntimeForTesting {
  port?: number
  token?: string
  runtimeMode?: InjectedRuntimeMode
}

let injectedRuntimeForTesting: InjectedRuntimeForTesting | undefined

/** Test-only: override embedded runtime globals without mutating `window`. */
export function setInjectedRuntimeForTesting(
  runtime: InjectedRuntimeForTesting | undefined,
): void {
  injectedRuntimeForTesting = runtime
}

export function injectedRuntimeMode(): InjectedRuntimeMode | undefined {
  if (injectedRuntimeForTesting) {
    return injectedRuntimeForTesting.runtimeMode
  }
  if (typeof window === 'undefined') {
    return undefined
  }
  const mode = (window as unknown as Record<string, unknown>)
    .__POSTHASTE_RUNTIME_MODE__
  return mode === 'loopback' || mode === 'native' ? mode : undefined
}

/** The embedded server's injected port, or `undefined` outside the bundled build. */
export function injectedPort(): number | undefined {
  if (injectedRuntimeForTesting) {
    return injectedRuntimeForTesting.port
  }
  if (typeof window === 'undefined') {
    return undefined
  }
  const port = (window as unknown as Record<string, unknown>).__POSTHASTE_PORT__
  return typeof port === 'number' ? port : undefined
}

/** The embedded server's injected bearer token, or `undefined` when absent. */
export function injectedToken(): string | undefined {
  if (injectedRuntimeForTesting) {
    const token = injectedRuntimeForTesting.token
    return typeof token === 'string' && token.length > 0 ? token : undefined
  }
  if (typeof window === 'undefined') {
    return undefined
  }
  const token = (window as unknown as Record<string, unknown>)
    .__POSTHASTE_TOKEN__
  return typeof token === 'string' && token.length > 0 ? token : undefined
}

/**
 * Resolve the embedded base URL exactly as the legacy `resolveBaseUrl()` did:
 * the injected port wins; otherwise the browser-dev fallback
 * (`VITE_API_BASE_URL` or `http://localhost:3001/v1`).
 */
export function injectedBaseUrl(): string {
  const port = injectedPort()
  if (port !== undefined) {
    return `http://127.0.0.1:${port}/v1`
  }
  return normalizeApiBaseUrl(
    import.meta.env.VITE_API_BASE_URL?.trim() || 'http://localhost:3001/v1',
  )
}
