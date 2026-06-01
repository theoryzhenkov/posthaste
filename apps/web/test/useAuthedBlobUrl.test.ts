import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { renderHook, waitFor } from '@testing-library/react'

import { useAuthedBlobUrl } from '../src/hooks/useAuthedBlobUrl'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

// Object-URL lifecycle tracking + fetch stubbing. We stub the globals the hook
// touches (fetch, URL.createObjectURL/revokeObjectURL) so we can assert the
// blob is created on success, never on error, and always revoked on teardown.
let created: string[] = []
let revoked: string[] = []
let fetchCalls = 0
let counter = 0

const originalFetch = globalThis.fetch
const originalCreate = URL.createObjectURL
const originalRevoke = URL.revokeObjectURL

function stubFetch(respond: () => Response): void {
  globalThis.fetch = (async () => {
    fetchCalls += 1
    return respond()
  }) as typeof fetch
}

beforeEach(() => {
  created = []
  revoked = []
  fetchCalls = 0
  counter = 0
  URL.createObjectURL = (() => {
    const url = `blob:mock/${(counter += 1)}`
    created.push(url)
    return url
  }) as typeof URL.createObjectURL
  URL.revokeObjectURL = ((url: string) => {
    revoked.push(url)
  }) as typeof URL.revokeObjectURL
})

afterEach(() => {
  globalThis.fetch = originalFetch
  URL.createObjectURL = originalCreate
  URL.revokeObjectURL = originalRevoke
})

describe('useAuthedBlobUrl', () => {
  it('is idle and fetches nothing for a null url', () => {
    stubFetch(() => new Response('bytes', { status: 200 }))
    const { result } = renderHook(() => useAuthedBlobUrl(null))
    expect(result.current.objectUrl).toBeNull()
    expect(result.current.isLoading).toBe(false)
    expect(result.current.error).toBeNull()
    expect(fetchCalls).toBe(0)
  })

  it('loads a url into an object URL and clears loading', async () => {
    stubFetch(() => new Response('bytes', { status: 200 }))
    const { result } = renderHook(() => useAuthedBlobUrl('/v1/logo'))
    expect(result.current.isLoading).toBe(true)
    await waitFor(() => expect(result.current.objectUrl).not.toBeNull())
    expect(result.current.objectUrl).toBe('blob:mock/1')
    expect(result.current.isLoading).toBe(false)
    expect(result.current.error).toBeNull()
    expect(created).toEqual(['blob:mock/1'])
  })

  it('surfaces an error on a non-ok response and creates no object URL', async () => {
    stubFetch(() => new Response('', { status: 403 }))
    const { result } = renderHook(() => useAuthedBlobUrl('/v1/logo'))
    await waitFor(() => expect(result.current.error).not.toBeNull())
    expect(result.current.objectUrl).toBeNull()
    expect(result.current.isLoading).toBe(false)
    expect(created).toEqual([])
  })

  it('revokes the object URL on unmount', async () => {
    stubFetch(() => new Response('bytes', { status: 200 }))
    const { result, unmount } = renderHook(() => useAuthedBlobUrl('/v1/logo'))
    await waitFor(() => expect(result.current.objectUrl).toBe('blob:mock/1'))
    unmount()
    expect(revoked).toEqual(['blob:mock/1'])
  })

  it('revokes the old URL and reloads when the input url changes', async () => {
    stubFetch(() => new Response('bytes', { status: 200 }))
    const { result, rerender, unmount } = renderHook(
      ({ url }: { url: string }) => useAuthedBlobUrl(url),
      { initialProps: { url: '/v1/a' } },
    )
    await waitFor(() => expect(result.current.objectUrl).toBe('blob:mock/1'))
    rerender({ url: '/v1/b' })
    // The effect cleanup revokes the previous URL synchronously on the change.
    expect(revoked).toContain('blob:mock/1')
    // While the new fetch is in flight the stale URL is not surfaced.
    expect(result.current.objectUrl).toBeNull()
    await waitFor(() => expect(result.current.objectUrl).toBe('blob:mock/2'))
    unmount()
    expect(revoked).toContain('blob:mock/2')
  })
})
