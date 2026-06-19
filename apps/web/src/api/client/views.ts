import { jsonRequest } from './core'

export interface OpenViewRequest {
  descriptor: {
    family: string
    payload: unknown
  }
}

export interface OpenViewResponse<TSnapshot = unknown> {
  viewId: string
  snapshot: TSnapshot
}

export function openView<TSnapshot = unknown>(
  input: OpenViewRequest,
  options?: { sourceId?: string | null },
): Promise<OpenViewResponse<TSnapshot>> {
  const params = new URLSearchParams()
  if (options?.sourceId) {
    params.set('sourceId', options.sourceId)
  }
  const search = params.toString()
  return jsonRequest<OpenViewResponse<TSnapshot>>(
    `/views${search ? `?${search}` : ''}`,
    'POST',
    input,
  )
}
