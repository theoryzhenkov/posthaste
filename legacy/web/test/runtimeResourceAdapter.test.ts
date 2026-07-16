import { afterEach, describe, expect, it } from 'bun:test'

import {
  applyResolvedConnection,
  resetActiveConnectionForTesting,
} from '../src/connection/runtime'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import { createFakeRuntimeAdapter } from '../src/runtime/fakeAdapter'
import { runtimeResources } from '../src/runtime/resources'

const originalFetch = globalThis.fetch

afterEach(() => {
  globalThis.fetch = originalFetch
  resetActiveConnectionForTesting()
  resetRuntimeAdapterForTesting()
})

describe('runtime resource adapter', () => {
  it('dispatches resource reads through a fake adapter override without a backend', async () => {
    const fake = createFakeRuntimeAdapter()
    const blob = new Blob(['bytes'])
    fake.queueResourceBlob(blob)
    setRuntimeAdapterForTesting(fake)

    const resource = { kind: 'account-logo' as const, imageId: 'logo-1' }
    const result = await runtimeResources.blob(resource)

    expect(result).toBe(blob)
    expect(fake.resourceCalls).toEqual([{ descriptor: resource }])
  })

  it('wraps existing HTTP resource reads by default without putting tokens in URLs', async () => {
    let capturedUrl = ''
    let capturedHeaders = new Headers()
    applyResolvedConnection(
      { baseUrl: 'http://127.0.0.1:4815/v1', token: 'resource-token' },
      null,
    )
    globalThis.fetch = (async (input, init) => {
      capturedUrl = String(input)
      capturedHeaders = new Headers(init?.headers)
      return new Response('bytes', { status: 200 })
    }) as typeof fetch

    const blob = await runtimeResources.blob({
      kind: 'message-attachment',
      sourceId: 'src 1',
      messageId: 'msg/2',
      attachmentId: 'att:3',
    })

    expect(await blob.text()).toBe('bytes')
    expect(capturedUrl).toBe(
      'http://127.0.0.1:4815/v1/sources/src%201/messages/msg%2F2/attachments/att%3A3',
    )
    expect(capturedUrl).not.toContain('resource-token')
    expect(capturedHeaders.get('Authorization')).toBe('Bearer resource-token')
  })
})
