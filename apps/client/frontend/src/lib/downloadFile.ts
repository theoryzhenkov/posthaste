/**
 * Download bytes from an authenticated URL (for example an attachment blob)
 * and hand them to the browser's save interaction under the given filename.
 * The blob endpoint carries its token in the URL, so a plain fetch works
 * from any origin the app runs on.
 */
import { LOG_EVENTS } from '../logEvents'
import { syncLogger } from '../logger'

export async function downloadFileFromUrl(
  url: string,
  filename: string,
): Promise<void> {
  try {
    const response = await fetch(url)
    if (!response.ok) {
      throw new Error(`blob fetch failed with status ${response.status}`)
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
      'failed to download file',
    )
    throw error
  }
}
