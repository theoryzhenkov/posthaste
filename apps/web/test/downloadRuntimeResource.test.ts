import { afterEach, beforeEach, describe, expect, it } from 'bun:test'

import { downloadRuntimeResource } from '../src/lib/downloadRuntimeResource'
import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import type { RuntimeResourceDescriptor } from '../src/runtime/types'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

let created: string[] = []
let revoked: string[] = []
let clicks: Array<{ href: string; download: string }> = []
let counter = 0
let runtimeAdapter: FakeRuntimeAdapter

const originalCreate = URL.createObjectURL
const originalRevoke = URL.revokeObjectURL
let originalCreateElement: typeof document.createElement

const attachmentResource: RuntimeResourceDescriptor = {
  kind: 'message-attachment',
  sourceId: 'primary',
  messageId: 'm1',
  attachmentId: 'a1',
}

const flushMacrotask = () => new Promise((resolve) => setTimeout(resolve, 5))

beforeEach(() => {
  created = []
  revoked = []
  clicks = []
  counter = 0
  runtimeAdapter = createFakeRuntimeAdapter()
  setRuntimeAdapterForTesting(runtimeAdapter)
  URL.createObjectURL = (() => {
    const url = `blob:mock/${(counter += 1)}`
    created.push(url)
    return url
  }) as typeof URL.createObjectURL
  URL.revokeObjectURL = ((url: string) => {
    revoked.push(url)
  }) as typeof URL.revokeObjectURL
  originalCreateElement = document.createElement.bind(document)
  document.createElement = ((tag: string) => {
    const element = originalCreateElement(tag)
    if (tag === 'a') {
      ;(element as HTMLAnchorElement).click = () => {
        const anchor = element as HTMLAnchorElement
        clicks.push({
          href: anchor.href,
          download: anchor.getAttribute('download') ?? '',
        })
      }
    }
    return element
  }) as typeof document.createElement
})

afterEach(() => {
  resetRuntimeAdapterForTesting()
  URL.createObjectURL = originalCreate
  URL.revokeObjectURL = originalRevoke
  document.createElement = originalCreateElement
})

describe('downloadRuntimeResource', () => {
  it('fetches via runtime adapter, clicks a download anchor, and revokes the object URL', async () => {
    runtimeAdapter.queueResourceBlob(new Blob(['bytes']))

    await downloadRuntimeResource(attachmentResource, 'doc.pdf')

    expect(runtimeAdapter.resourceCalls).toEqual([
      { descriptor: attachmentResource },
    ])
    expect(created).toEqual(['blob:mock/1'])
    expect(clicks).toEqual([{ href: 'blob:mock/1', download: 'doc.pdf' }])

    expect(revoked).toEqual([])
    await flushMacrotask()
    expect(revoked).toEqual(['blob:mock/1'])
  })

  it('throws adapter errors and never creates an object URL', async () => {
    runtimeAdapter.queueResourceError(new Error('download failed'))

    await expect(
      downloadRuntimeResource(attachmentResource, 'doc.pdf'),
    ).rejects.toThrow('download failed')
    expect(created).toEqual([])
    expect(clicks).toEqual([])
  })
})
