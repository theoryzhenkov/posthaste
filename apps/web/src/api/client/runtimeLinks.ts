import { jsonRequest } from './core'

export interface OpenRuntimeLinkResponse {
  linkId: string
}

export interface OpenRuntimeLinkViewRequest {
  descriptor: {
    family: string
    payload: unknown
  }
}

export interface OpenRuntimeLinkViewResponse<TSnapshot = unknown> {
  viewId: string
  snapshot: TSnapshot
}

export interface RunRuntimeMutationRequest {
  linkId?: string | null
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

export function openRuntimeLink(options?: {
  sourceId?: string | null
  /** Opt into incremental mail-list view deltas (replication client-link). */
  viewDelta?: boolean
}): Promise<OpenRuntimeLinkResponse> {
  const params = new URLSearchParams()
  if (options?.sourceId) {
    params.set('sourceId', options.sourceId)
  }
  if (options?.viewDelta) {
    params.set('viewDelta', 'true')
  }
  const search = params.toString()
  return jsonRequest<OpenRuntimeLinkResponse>(
    `/runtime/sessions${search ? `?${search}` : ''}`,
    'POST',
  )
}

export function openRuntimeLinkView<TSnapshot = unknown>(
  linkId: string,
  input: OpenRuntimeLinkViewRequest,
  options?: { sourceId?: string | null },
): Promise<OpenRuntimeLinkViewResponse<TSnapshot>> {
  return jsonRequest<OpenRuntimeLinkViewResponse<TSnapshot>>(
    `/runtime/sessions/${encodeURIComponent(linkId)}/views${sourceSearch(options?.sourceId)}`,
    'POST',
    input,
  )
}

export function extendRuntimeLinkView<TSnapshot = unknown>(
  linkId: string,
  viewId: string,
  count: number,
  options?: { sourceId?: string | null },
): Promise<OpenRuntimeLinkViewResponse<TSnapshot>> {
  return jsonRequest<OpenRuntimeLinkViewResponse<TSnapshot>>(
    `/runtime/sessions/${encodeURIComponent(linkId)}/views/${encodeURIComponent(viewId)}/extend${sourceSearch(options?.sourceId)}`,
    'POST',
    { count },
  )
}

export function closeRuntimeLink(
  linkId: string,
  options?: { sourceId?: string | null },
): Promise<{ ok: true }> {
  return jsonRequest<{ ok: true }>(
    `/runtime/sessions/${encodeURIComponent(linkId)}${sourceSearch(options?.sourceId)}`,
    'DELETE',
  )
}

export function closeRuntimeLinkView(
  linkId: string,
  viewId: string,
  options?: { sourceId?: string | null },
): Promise<{ ok: true }> {
  return jsonRequest<{ ok: true }>(
    `/runtime/sessions/${encodeURIComponent(linkId)}/views/${encodeURIComponent(viewId)}${sourceSearch(options?.sourceId)}`,
    'DELETE',
  )
}

export function runRuntimeMutation(
  linkId: string,
  input: RunRuntimeMutationRequest,
  options?: { sourceId?: string | null },
): Promise<RunRuntimeMutationResponse> {
  return jsonRequest<RunRuntimeMutationResponse>(
    `/runtime/sessions/${encodeURIComponent(linkId)}/mutations${sourceSearch(options?.sourceId)}`,
    'POST',
    { ...input, linkId },
  )
}
