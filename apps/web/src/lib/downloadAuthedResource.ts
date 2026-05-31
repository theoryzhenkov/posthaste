/**
 * Download an auth-gated daemon resource (e.g. a message attachment).
 *
 * The browser can't attach the `Authorization` header to an `<a download href>`
 * navigation, so we `fetch()` the bytes with the header, then trigger a save
 * from the resulting object URL. This is the imperative, lazy counterpart to
 * {@link useAuthedBlobUrl}: call it on click so a list of attachments isn't all
 * fetched eagerly just to render download buttons.
 *
 * @spec docs/eph/DESIGN-L1-trust-model
 */
import { authHeaders } from '../api/client'
import { syncLogger } from '../logger'
import { LOG_EVENTS } from '../logEvents'

/**
 * Fetch `url` with the active connection's auth headers and save it as
 * `filename`. Logs and rethrows on failure so callers can surface it; a
 * fire-and-forget caller can `.catch(() => {})` since it is already logged.
 */
export async function downloadAuthedResource(
  url: string,
  filename: string,
): Promise<void> {
  try {
    const response = await fetch(url, { headers: authHeaders() })
    if (!response.ok) {
      throw new Error(`download failed with ${response.status}`)
    }
    const blob = await response.blob()
    const objectUrl = URL.createObjectURL(blob)
    const anchor = document.createElement('a')
    anchor.href = objectUrl
    anchor.download = filename
    document.body.appendChild(anchor)
    anchor.click()
    anchor.remove()
    // Defer revoke to the next tick: revoking synchronously can cancel the
    // download before the browser has read the blob in some engines.
    setTimeout(() => URL.revokeObjectURL(objectUrl), 0)
  } catch (error) {
    syncLogger.warn(
      { event: LOG_EVENTS.resourceFetchError, error, url },
      'failed to download authenticated resource',
    )
    throw error
  }
}
