/**
 * Download runtime-owned resource bytes (for example a message attachment).
 *
 * The runtime adapter owns transport details such as loopback URLs and bearer
 * headers. The UI owns the filename and the browser save interaction.
 */
import { LOG_EVENTS } from '../logEvents'
import { syncLogger } from '../logger'
import { fetchRuntimeResourceBlob } from '../runtime/adapter'
import type { RuntimeResourceDescriptor } from '../runtime/types'

export async function downloadRuntimeResource(
  resource: RuntimeResourceDescriptor,
  filename: string,
): Promise<void> {
  try {
    const blob = await fetchRuntimeResourceBlob(resource)
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
      { event: LOG_EVENTS.resourceFetchError, error, resource },
      'failed to download runtime resource',
    )
    throw error
  }
}
