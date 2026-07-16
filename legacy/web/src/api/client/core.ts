import { getActiveConnection } from '../../connection/runtime'
import { apiLogger } from '../../logger'
import { LOG_EVENTS } from '../../logEvents'
import {
  createOperationContext,
  createRequestContext,
  observabilityHeaders,
  type OperationContext,
} from '../../observability'

import { ApiError } from '../errors'
import type { ApiErrorCode } from '../types'

type RequestInitWithObservability = RequestInit & {
  operation?: OperationContext
}

/**
 * The `/v1` base URL of the active connection. Dynamic (per active profile),
 * not a module-load const: reads the runtime holder, which is seeded to the
 * embedded injection (`__POSTHASTE_PORT__`, or the browser-dev fallback) so the
 * bundled build is unchanged, and re-pointed on a profile switch.
 *
 * @spec docs/eph/DESIGN-L1-deployment-modes#connection-profiles
 */
export function baseUrl(): string {
  return getActiveConnection().baseUrl
}

/**
 * The per-connection bearer token. For the embedded profile this is the
 * injected `__POSTHASTE_TOKEN__` (today's behavior); for remote profiles it is
 * the keyring-held token. Absent when the server does not require auth.
 *
 * @spec docs/eph/DESIGN-L1-trust-model
 */
function authToken(): string | undefined {
  return getActiveConnection().token
}

/** The pinned `Host` header for a remote profile, or `undefined`. */
function hostHeader(): string | undefined {
  return getActiveConnection().hostHeader
}

/**
 * Per-request headers carrying the bearer token (and, for remote profiles, the
 * pinned `Host` header). Empty when no token is set.
 *
 * Exported because the SSE stream (via `fetchEventSource`) and the
 * browser-loadable blob fetches (logos, attachments) authenticate with the
 * same header set rather than a URL token — there is no `access_token` query
 * param anywhere anymore.
 */
export function authHeaders(): Record<string, string> {
  const headers: Record<string, string> = {}
  const token = authToken()
  if (token) {
    headers.Authorization = `Bearer ${token}`
  }
  const host = hostHeader()
  if (host) {
    headers.Host = host
  }
  return headers
}

/** Parse a non-OK response into a structured {@link ApiError}. */
async function parseError(response: Response): Promise<never> {
  let message = response.statusText
  let code: ApiErrorCode | undefined

  try {
    const payload = (await response.json()) as {
      code?: ApiErrorCode
      message?: string
    }
    message = payload.message ?? message
    code = payload.code
  } catch {
    // Preserve the HTTP status text when the body is not JSON.
  }

  throw new ApiError(response.status, response.statusText, message, code)
}

function safeLogPath(path: string): string {
  return path.split('?')[0] ?? path
}

function mergeHeaders(
  base: HeadersInit | undefined,
  extra: HeadersInit,
): Headers {
  const headers = new Headers(base)
  for (const [key, value] of Object.entries(extra)) {
    headers.set(key, value)
  }
  return headers
}

/**
 * Cap concurrent API requests.
 *
 * The Tauri macOS webview (WKWebView) talks to the embedded daemon over HTTP/1.1
 * on loopback, which allows only ~6 connections per host; SSE streams hold some
 * of those for their lifetime. A burst of fetches beyond that limit — the
 * conversation view opening many threads at once is the worst offender — makes
 * WKWebView drop connections, surfacing as "network connection was lost" and
 * "access control checks" (a dropped/early-closed response arrives without the
 * CORS headers). React Query retries, so requests still succeed, but the console
 * fills with errors. Queue the overflow client-side instead so the webview never
 * sees more than a handful of in-flight requests. Harmless in real browsers,
 * which already queue per host. SSE (`fetchEventSource`) and blob downloads use
 * their own fetches and are intentionally not gated.
 */
const MAX_CONCURRENT_REQUESTS = 4
/// How long a request may wait for a concurrency slot before failing fast. If
/// all in-flight requests hang (a server that stopped responding), a parked
/// waiter must surface a bounded failure rather than hang forever (engineering
/// principle VI). Generous: normal operation never waits; this only fires when
/// the queue is genuinely wedged. Held as a `let` so a test can shrink it via
/// [`setRequestSlotTimeoutMsForTesting`] (principle II: one declared seam a
/// test can reach).
let requestSlotTimeoutMs = 30_000

/** Test-only: shrink the slot-acquisition deadline so a wedged queue triggers
 * the timeout in milliseconds, not 30s. Returns a guard that restores it. */
export function setRequestSlotTimeoutMsForTesting(ms: number): () => void {
  const previous = requestSlotTimeoutMs
  requestSlotTimeoutMs = ms
  return () => {
    requestSlotTimeoutMs = previous
  }
}
let inFlightRequests = 0
/// A parked waiter for a concurrency slot: resolves when `releaseRequestSlot`
/// hands a slot over, or rejects when the acquisition deadline fires.
interface RequestWaiter {
  resolve: () => void
  reject: (error: ApiError) => void
  timer: ReturnType<typeof setTimeout>
}
const requestWaiters: RequestWaiter[] = []

function acquireRequestSlot(): Promise<void> {
  if (inFlightRequests < MAX_CONCURRENT_REQUESTS) {
    inFlightRequests += 1
    return Promise.resolve()
  }
  return new Promise<void>((resolve, reject) => {
    const waiter: RequestWaiter = {
      resolve,
      reject,
      timer: undefined as never,
    }
    requestWaiters.push(waiter)
    // Bound the wait: if no slot frees up within the deadline, remove the
    // waiter from the queue and reject so the caller fails fast (and React
    // Query can surface the error) instead of parking indefinitely.
    waiter.timer = setTimeout(() => {
      const index = requestWaiters.indexOf(waiter)
      if (index !== -1) {
        requestWaiters.splice(index, 1)
      }
      reject(
        new ApiError(
          0,
          'Request Queue Timeout',
          `no concurrency slot acquired within ${requestSlotTimeoutMs / 1000}s; the server appears unresponsive`,
        ),
      )
    }, requestSlotTimeoutMs)
  })
}

function releaseRequestSlot(): void {
  const next = requestWaiters.shift()
  if (next) {
    // Hand the slot straight to the next waiter; in-flight count is unchanged.
    // Clear the acquisition deadline so it cannot fire after the hand-off.
    clearTimeout(next.timer)
    next.resolve()
  } else {
    inFlightRequests -= 1
  }
}

async function withRequestSlot<T>(run: () => Promise<T>): Promise<T> {
  await acquireRequestSlot()
  try {
    return await run()
  } finally {
    releaseRequestSlot()
  }
}

/** Low-level fetch wrapper that throws {@link ApiError} on non-OK responses. */
export async function request<T>(
  path: string,
  init: RequestInitWithObservability = {},
): Promise<T> {
  const { operation, headers, ...fetchInit } = init
  const context = createRequestContext(
    operation ?? createOperationContext('api.request', 'api-client'),
  )
  const requestPath = safeLogPath(path)
  const method = fetchInit.method ?? 'GET'
  const started = performance.now()
  apiLogger.debug(
    {
      event: LOG_EVENTS.apiRequestStarted,
      requestId: context.requestId,
      operationId: context.operationId,
      operationKind: context.operationKind,
      operationSource: context.operationSource,
      sessionId: context.sessionId,
      method,
      path: requestPath,
    },
    'api request started',
  )
  return withRequestSlot(async () => {
    let response: Response
    try {
      response = await fetch(`${baseUrl()}${path}`, {
        ...fetchInit,
        headers: mergeHeaders(headers, {
          ...observabilityHeaders(context),
          ...authHeaders(),
        }),
      })
    } catch (error) {
      apiLogger.warn(
        {
          event: LOG_EVENTS.apiRequestFailed,
          requestId: context.requestId,
          operationId: context.operationId,
          operationKind: context.operationKind,
          operationSource: context.operationSource,
          sessionId: context.sessionId,
          method,
          path: requestPath,
          durationMs: Math.round(performance.now() - started),
          error,
        },
        'api request failed before response',
      )
      throw error
    }
    apiLogger.debug(
      {
        event: LOG_EVENTS.apiRequestCompleted,
        requestId: context.requestId,
        operationId: context.operationId,
        operationKind: context.operationKind,
        operationSource: context.operationSource,
        sessionId: context.sessionId,
        method,
        path: requestPath,
        status: response.status,
        durationMs: Math.round(performance.now() - started),
      },
      'api request completed',
    )
    if (!response.ok) {
      return parseError(response)
    }
    return (await response.json()) as T
  })
}

/** Convenience wrapper for JSON-bodied requests (POST / PATCH). */
export function jsonRequest<T>(
  path: string,
  method: string,
  body?: unknown,
  operation?: OperationContext,
): Promise<T> {
  return request<T>(path, {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    operation,
  })
}
