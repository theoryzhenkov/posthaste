import type { ApiErrorKind } from '@/gen'

/**
 * Stable machine-readable API error code.
 *
 * Pure alias of the generated wire enum: there is no curation here, so it needs
 * no conformance assertion — it IS the wire type.
 */
export type ApiErrorCode = ApiErrorKind

export interface OkResponse {
  ok: boolean
}

export type { SyncMode } from '@/gen'
