import { jsonRequest } from './core'

export interface OpenRuntimeSessionResponse {
  sessionId: string
}

export interface OpenRuntimeSessionViewRequest {
  descriptor: {
    family: string
    payload: unknown
  }
}

export interface OpenRuntimeSessionViewResponse<TSnapshot = unknown> {
  viewId: string
  snapshot: TSnapshot
}

export interface RunRuntimeMutationRequest {
  sessionId?: string | null
  name: string
  args?: unknown
  clientMutationId: string
  context?: unknown
}

export type RuntimeMutationSettlementStatus =
  | 'accepted'
  | 'localApplied'
  | 'queued'
  | 'confirmed'
  | 'failed'
  | 'conflict'

export interface RuntimeAdapterErrorResponse {
  code: string
  message: string
  retryable: boolean
  correlationId?: string | null
  details?: unknown
}

export interface RunRuntimeMutationResponse {
  runtimeMutationId: string | null
  clientMutationId: string
  name: string
  state: RuntimeMutationSettlementStatus
  error: RuntimeAdapterErrorResponse | null
  output?: unknown
}

function sourceSearch(sourceId?: string | null): string {
  const params = new URLSearchParams()
  if (sourceId) {
    params.set('sourceId', sourceId)
  }
  const search = params.toString()
  return search ? `?${search}` : ''
}

export function openRuntimeSession(options?: {
  sourceId?: string | null
  /** Opt into incremental mail-list view deltas (replication client-link). */
  viewDelta?: boolean
}): Promise<OpenRuntimeSessionResponse> {
  const params = new URLSearchParams()
  if (options?.sourceId) {
    params.set('sourceId', options.sourceId)
  }
  if (options?.viewDelta) {
    params.set('viewDelta', 'true')
  }
  const search = params.toString()
  return jsonRequest<OpenRuntimeSessionResponse>(
    `/runtime/sessions${search ? `?${search}` : ''}`,
    'POST',
  )
}

export function openRuntimeSessionView<TSnapshot = unknown>(
  sessionId: string,
  input: OpenRuntimeSessionViewRequest,
  options?: { sourceId?: string | null },
): Promise<OpenRuntimeSessionViewResponse<TSnapshot>> {
  return jsonRequest<OpenRuntimeSessionViewResponse<TSnapshot>>(
    `/runtime/sessions/${encodeURIComponent(sessionId)}/views${sourceSearch(options?.sourceId)}`,
    'POST',
    input,
  )
}

export function extendRuntimeSessionView<TSnapshot = unknown>(
  sessionId: string,
  viewId: string,
  count: number,
  options?: { sourceId?: string | null },
): Promise<OpenRuntimeSessionViewResponse<TSnapshot>> {
  return jsonRequest<OpenRuntimeSessionViewResponse<TSnapshot>>(
    `/runtime/sessions/${encodeURIComponent(sessionId)}/views/${encodeURIComponent(viewId)}/extend${sourceSearch(options?.sourceId)}`,
    'POST',
    { count },
  )
}

export function closeRuntimeSession(
  sessionId: string,
  options?: { sourceId?: string | null },
): Promise<{ ok: true }> {
  return jsonRequest<{ ok: true }>(
    `/runtime/sessions/${encodeURIComponent(sessionId)}${sourceSearch(options?.sourceId)}`,
    'DELETE',
  )
}

export function closeRuntimeSessionView(
  sessionId: string,
  viewId: string,
  options?: { sourceId?: string | null },
): Promise<{ ok: true }> {
  return jsonRequest<{ ok: true }>(
    `/runtime/sessions/${encodeURIComponent(sessionId)}/views/${encodeURIComponent(viewId)}${sourceSearch(options?.sourceId)}`,
    'DELETE',
  )
}

export function runRuntimeMutation(
  sessionId: string,
  input: RunRuntimeMutationRequest,
  options?: { sourceId?: string | null },
): Promise<RunRuntimeMutationResponse> {
  return jsonRequest<RunRuntimeMutationResponse>(
    `/runtime/sessions/${encodeURIComponent(sessionId)}/mutations${sourceSearch(options?.sourceId)}`,
    'POST',
    { ...input, sessionId },
  )
}
