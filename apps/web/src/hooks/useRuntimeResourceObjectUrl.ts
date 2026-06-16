/**
 * Fetch runtime-owned resource bytes and expose them as an object URL for
 * `<img>` / `<iframe>` / `<a download>`.
 *
 * The renderer asks the runtime adapter for resource bytes by descriptor. The
 * temporary HTTP bridge can attach bearer headers internally; components never
 * handle transport URLs or tokens.
 */
import { useEffect, useState } from 'react'

import { LOG_EVENTS } from '../logEvents'
import { syncLogger } from '../logger'
import { fetchRuntimeResourceBlob } from '../runtime/adapter'
import type { RuntimeResourceDescriptor } from '../runtime/types'

export interface RuntimeResourceObjectUrl {
  /** Object URL for the fetched blob, or `null` while loading / on error. */
  objectUrl: string | null
  isLoading: boolean
  error: Error | null
}

interface BlobResult {
  key: string | null
  objectUrl: string | null
  error: Error | null
}

function runtimeResourceKey(resource: RuntimeResourceDescriptor): string {
  switch (resource.kind) {
    case 'account-logo':
      return JSON.stringify(['account-logo', resource.imageId])
    case 'message-attachment':
      return JSON.stringify([
        'message-attachment',
        resource.sourceId,
        resource.messageId,
        resource.attachmentId,
      ])
  }
}

function runtimeResourceFromKey(key: string): RuntimeResourceDescriptor {
  const [kind, first, second, third] = JSON.parse(key) as string[]
  if (kind === 'account-logo') {
    return { kind, imageId: first ?? '' }
  }
  return {
    kind: 'message-attachment',
    sourceId: first ?? '',
    messageId: second ?? '',
    attachmentId: third ?? '',
  }
}

export function useRuntimeResourceObjectUrl(
  resource: RuntimeResourceDescriptor | null | undefined,
): RuntimeResourceObjectUrl {
  const targetKey = resource ? runtimeResourceKey(resource) : null
  const [result, setResult] = useState<BlobResult>({
    key: null,
    objectUrl: null,
    error: null,
  })

  useEffect(() => {
    if (targetKey == null) {
      return
    }
    const target = runtimeResourceFromKey(targetKey)

    const controller = new AbortController()
    let objectUrl: string | null = null

    void (async () => {
      try {
        const blob = await fetchRuntimeResourceBlob(target, {
          signal: controller.signal,
        })
        if (controller.signal.aborted) {
          return
        }
        objectUrl = URL.createObjectURL(blob)
        setResult({ key: targetKey, objectUrl, error: null })
      } catch (error) {
        if (controller.signal.aborted) {
          return
        }
        syncLogger.warn(
          { event: LOG_EVENTS.resourceFetchError, error, resource: target },
          'failed to load runtime resource',
        )
        setResult({
          key: targetKey,
          objectUrl: null,
          error: error instanceof Error ? error : new Error(String(error)),
        })
      }
    })()

    return () => {
      controller.abort()
      if (objectUrl) {
        URL.revokeObjectURL(objectUrl)
      }
    }
  }, [targetKey])

  const settled = result.key === targetKey
  return {
    objectUrl: settled ? result.objectUrl : null,
    isLoading: targetKey != null && !settled,
    error: settled ? result.error : null,
  }
}
