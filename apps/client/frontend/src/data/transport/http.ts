// The HTTP unit of the transport: one origin, one bearer token, one
// injectable fetch. Owns request formatting, the typed error envelope, and
// the token-in-URL rules for GETs that plain attributes issue (anchors,
// <img>, EventSource cannot set headers). Nothing here knows about queries,
// commands, or the stream — callers bring paths and bodies.

import type { ApiError } from '@/gen'

/** The fetch shape the transport needs; injectable for tests. */
export type FetchLike = (input: string, init?: RequestInit) => Promise<Response>

/** A failed HTTP call, carrying the typed error envelope fields. */
export class MailApiError extends Error {
  readonly kind: ApiError['kind']
  readonly retryable: boolean
  readonly httpStatus: number

  constructor(err: ApiError, httpStatus: number) {
    super(err.message)
    this.name = 'MailApiError'
    this.kind = err.kind
    this.retryable = err.retryable
    this.httpStatus = httpStatus
  }
}

export interface HttpTransportOptions {
  /** Origin prefix for every request; '' when served behind the dev proxy. */
  baseUrl: string
  /** Session secret or capability token; sent as a bearer header on POSTs. */
  token: string
  fetchImpl?: FetchLike
}

export class HttpTransport {
  private readonly baseUrl: string
  private readonly token: string
  private readonly fetchImpl: FetchLike

  constructor(opts: HttpTransportOptions) {
    this.baseUrl = opts.baseUrl.replace(/\/$/, '')
    this.token = opts.token
    this.fetchImpl = opts.fetchImpl ?? ((input, init) => fetch(input, init))
  }

  /** POSTs a pre-serialized JSON body with the bearer header; a non-OK
   * response becomes a MailApiError (or a generic error when the body is not
   * the typed envelope). Returns the parsed response body. */
  async postJson(path: string, body: string): Promise<unknown> {
    const res = await this.fetchImpl(`${this.baseUrl}${path}`, {
      method: 'POST',
      headers: {
        'content-type': 'application/json',
        authorization: `Bearer ${this.token}`,
      },
      body,
    })
    if (!res.ok) throw await this.errorFrom(res)
    return (await res.json()) as unknown
  }

  /** Authenticated URL for a GET issued from an href/src attribute. The
   * token rides as a query parameter and is omitted when empty (the dev
   * proxy injects the Authorization header instead). */
  getUrl(path: string): string {
    const token = this.token ? `?token=${encodeURIComponent(this.token)}` : ''
    return `${this.baseUrl}${path}${token}`
  }

  /** The event stream URL; the token always rides as a query parameter
   * because EventSource cannot set headers. */
  streamUrl(): string {
    return `${this.baseUrl}/events?token=${encodeURIComponent(this.token)}`
  }

  private async errorFrom(res: Response): Promise<Error> {
    try {
      const body = (await res.json()) as ApiError
      if (body && typeof body.message === 'string') return new MailApiError(body, res.status)
    } catch {
      // fall through to the generic error
    }
    return new Error(`request failed with HTTP ${res.status}`)
  }
}
