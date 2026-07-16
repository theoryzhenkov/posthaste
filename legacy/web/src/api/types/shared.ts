import type { components } from '../schema.gen'

/**
 * Stable machine-readable API error code.
 *
 * Pure alias of the generated wire enum: there is no curation here, so it needs
 * no conformance assertion — it IS the wire type.
 *
 * @spec docs/L1-api#error-format
 */
export type ApiErrorCode = components['schemas']['ApiErrorCode']

export interface OkResponse {
  ok: boolean
}

export type SyncMode = 'incremental' | 'fullMetadata'
