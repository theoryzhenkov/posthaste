/**
 * Fetch an authenticated daemon resource and expose it as an object URL for
 * `<img>` / `<iframe>` / `<a download>`.
 *
 * The browser cannot attach the `Authorization` header to a resource it loads
 * itself (an `<img src>` or download navigation), which is why these used to
 * carry the token in an `?access_token=` query param. Instead we `fetch()` the
 * bytes ourselves with {@link authHeaders}, wrap them in a blob, and hand back
 * an object URL. The object URL is revoked — and the in-flight request aborted —
 * when the input URL changes or the component unmounts, so blobs do not leak.
 *
 * @spec docs/eph/DESIGN-L1-trust-model
 */
import { useEffect, useState } from 'react'
import { authHeaders } from '../api/client'
import { syncLogger } from '../logger'
import { LOG_EVENTS } from '../logEvents'

export interface AuthedBlobUrl {
  /** Object URL for the fetched blob, or `null` while loading / on error. */
  objectUrl: string | null
  isLoading: boolean
  error: Error | null
}

/** Fetch outcome tagged with the URL it belongs to, so a stale result (from a
 * previous URL) is ignored until the in-flight fetch for the current URL lands. */
interface BlobResult {
  url: string | null
  objectUrl: string | null
  error: Error | null
}

/**
 * Load `url` with the active connection's auth headers and return it as an
 * object URL. Pass `null`/`undefined` to load nothing (e.g. before the resource
 * id is known).
 */
export function useAuthedBlobUrl(
  url: string | null | undefined,
): AuthedBlobUrl {
  const target = url ?? null
  // Only ever written asynchronously (after the fetch resolves) — never
  // synchronously in the effect body — so the loading/reset state is *derived*
  // below by comparing the resolved result's URL against the current target.
  const [result, setResult] = useState<BlobResult>({
    url: null,
    objectUrl: null,
    error: null,
  })

  useEffect(() => {
    if (target == null) {
      return
    }

    const controller = new AbortController()
    let objectUrl: string | null = null

    void (async () => {
      try {
        const response = await fetch(target, {
          headers: authHeaders(),
          signal: controller.signal,
        })
        if (!response.ok) {
          throw new Error(`resource fetch failed with ${response.status}`)
        }
        const blob = await response.blob()
        if (controller.signal.aborted) {
          return
        }
        objectUrl = URL.createObjectURL(blob)
        setResult({ url: target, objectUrl, error: null })
      } catch (error) {
        if (controller.signal.aborted) {
          return
        }
        syncLogger.warn(
          { event: LOG_EVENTS.resourceFetchError, error, url: target },
          'failed to load authenticated resource',
        )
        setResult({
          url: target,
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
  }, [target])

  // The stored result is only valid for the URL it was fetched for. For any
  // other target (a fresh URL, or null) we are still loading / idle and must
  // not surface the old (now-revoked) object URL.
  const settled = result.url === target
  return {
    objectUrl: settled ? result.objectUrl : null,
    isLoading: target != null && !settled,
    error: settled ? result.error : null,
  }
}
