import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { renderHook, waitFor } from '@testing-library/react'

import {
  resetRuntimeAdapterForTesting,
  setRuntimeAdapterForTesting,
} from '../src/runtime/adapter'
import {
  createFakeRuntimeAdapter,
  type FakeRuntimeAdapter,
} from '../src/runtime/fakeAdapter'
import { useRuntimeResourceObjectUrl } from '../src/hooks/useRuntimeResourceObjectUrl'
import type { RuntimeResourceDescriptor } from '../src/runtime/types'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

let created: string[] = []
let revoked: string[] = []
let counter = 0
let runtimeAdapter: FakeRuntimeAdapter

const originalCreate = URL.createObjectURL
const originalRevoke = URL.revokeObjectURL

const logoResource: RuntimeResourceDescriptor = {
  kind: 'account-logo',
  imageId: 'logo-1',
}

const otherLogoResource: RuntimeResourceDescriptor = {
  kind: 'account-logo',
  imageId: 'logo-2',
}

beforeEach(() => {
  created = []
  revoked = []
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
})

afterEach(() => {
  resetRuntimeAdapterForTesting()
  URL.createObjectURL = originalCreate
  URL.revokeObjectURL = originalRevoke
})

describe('useRuntimeResourceObjectUrl', () => {
  it('is idle and fetches nothing for a null resource', () => {
    const { result } = renderHook(() => useRuntimeResourceObjectUrl(null))

    expect(result.current.objectUrl).toBeNull()
    expect(result.current.isLoading).toBe(false)
    expect(result.current.error).toBeNull()
    expect(runtimeAdapter.resourceCalls).toEqual([])
  })

  it('loads a runtime resource into an object URL and clears loading', async () => {
    runtimeAdapter.queueResourceBlob(new Blob(['bytes']))

    const { result } = renderHook(() =>
      useRuntimeResourceObjectUrl(logoResource),
    )

    expect(result.current.isLoading).toBe(true)
    await waitFor(() => expect(result.current.objectUrl).not.toBeNull())
    expect(result.current.objectUrl).toBe('blob:mock/1')
    expect(result.current.isLoading).toBe(false)
    expect(result.current.error).toBeNull()
    expect(created).toEqual(['blob:mock/1'])
    expect(runtimeAdapter.resourceCalls).toEqual([{ descriptor: logoResource }])
  })

  it('surfaces an adapter error and creates no object URL', async () => {
    runtimeAdapter.queueResourceError(new Error('no resource'))

    const { result } = renderHook(() =>
      useRuntimeResourceObjectUrl(logoResource),
    )

    await waitFor(() => expect(result.current.error).not.toBeNull())
    expect(result.current.objectUrl).toBeNull()
    expect(result.current.isLoading).toBe(false)
    expect(created).toEqual([])
  })

  it('revokes the object URL on unmount', async () => {
    runtimeAdapter.queueResourceBlob(new Blob(['bytes']))

    const { result, unmount } = renderHook(() =>
      useRuntimeResourceObjectUrl(logoResource),
    )

    await waitFor(() => expect(result.current.objectUrl).toBe('blob:mock/1'))
    unmount()
    expect(revoked).toEqual(['blob:mock/1'])
  })

  it('revokes the old URL and reloads when the descriptor changes', async () => {
    runtimeAdapter.queueResourceBlob(new Blob(['a']))
    runtimeAdapter.queueResourceBlob(new Blob(['b']))
    const { result, rerender, unmount } = renderHook(
      ({ resource }: { resource: RuntimeResourceDescriptor }) =>
        useRuntimeResourceObjectUrl(resource),
      { initialProps: { resource: logoResource } },
    )
    await waitFor(() => expect(result.current.objectUrl).toBe('blob:mock/1'))

    rerender({ resource: otherLogoResource })

    expect(revoked).toContain('blob:mock/1')
    expect(result.current.objectUrl).toBeNull()
    await waitFor(() => expect(result.current.objectUrl).toBe('blob:mock/2'))
    unmount()
    expect(revoked).toContain('blob:mock/2')
  })
})
