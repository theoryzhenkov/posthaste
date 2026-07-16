import type { ApiErrorCode } from './types'

/**
 * Structured API error carrying HTTP status and an optional backend error code.
 * @spec docs/L1-api#error-format
 */
export class ApiError extends Error {
  readonly status: number
  readonly statusText: string
  readonly code?: ApiErrorCode

  constructor(
    status: number,
    statusText: string,
    message?: string,
    code?: ApiErrorCode,
  ) {
    super(message ?? `API error: ${status} ${statusText}`)
    this.name = 'ApiError'
    this.status = status
    this.statusText = statusText
    this.code = code
  }
}
