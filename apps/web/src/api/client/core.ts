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
  return response.json() as Promise<T>
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
