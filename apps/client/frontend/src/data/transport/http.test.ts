// The HTTP unit in isolation: request formatting (auth header, JSON body),
// error-envelope mapping, and the token-in-URL rules.

import { describe, expect, test } from 'bun:test'
import { HttpTransport, MailApiError } from './http'

function transport(handler: (url: string, init?: RequestInit) => Response | Promise<Response>) {
  return new HttpTransport({
    baseUrl: 'http://api.test/',
    token: 'tok',
    fetchImpl: async (url, init) => handler(url, init),
  })
}

describe('postJson', () => {
  test('posts the body verbatim with bearer auth and content-type', async () => {
    let seen: { url: string; init?: RequestInit } | null = null
    const t = transport((url, init) => {
      seen = { url, init }
      return Response.json({ ok: true })
    })
    const json = await t.postJson('/api/query', '{"accounts":{}}')
    expect(json).toEqual({ ok: true })
    expect(seen!.url).toBe('http://api.test/api/query') // trailing slash stripped
    expect(seen!.init?.method).toBe('POST')
    expect(seen!.init?.body).toBe('{"accounts":{}}')
    const headers = seen!.init?.headers as Record<string, string>
    expect(headers['authorization']).toBe('Bearer tok')
    expect(headers['content-type']).toBe('application/json')
  })

  test('a typed error envelope becomes a MailApiError with its fields', async () => {
    const t = transport(() =>
      Response.json(
        { kind: 'conflict', message: 'mailbox is not empty', retryable: false },
        { status: 409 },
      ),
    )
    const err = await t.postJson('/api/command', '{}').catch((e: unknown) => e)
    expect(err).toBeInstanceOf(MailApiError)
    const apiErr = err as MailApiError
    expect(apiErr.kind).toBe('conflict')
    expect(apiErr.message).toBe('mailbox is not empty')
    expect(apiErr.retryable).toBe(false)
    expect(apiErr.httpStatus).toBe(409)
  })

  test('a non-envelope failure becomes a generic error with the status', async () => {
    const t = transport(() => new Response('gateway timeout', { status: 504 }))
    await expect(t.postJson('/api/query', '{}')).rejects.toThrow(
      'request failed with HTTP 504',
    )
  })
})

describe('urls', () => {
  test('getUrl appends the token as a query parameter', () => {
    const t = transport(() => Response.json({}))
    expect(t.getUrl('/api/blobs/b1')).toBe('http://api.test/api/blobs/b1?token=tok')
  })

  test('getUrl omits the token when empty (dev proxy injects the header)', () => {
    const t = new HttpTransport({ baseUrl: '', token: '', fetchImpl: async () => Response.json({}) })
    expect(t.getUrl('/api/blobs/b1')).toBe('/api/blobs/b1')
  })

  test('streamUrl always carries the token parameter', () => {
    const t = new HttpTransport({ baseUrl: '', token: '', fetchImpl: async () => Response.json({}) })
    expect(t.streamUrl()).toBe('/events?token=')
  })
})
