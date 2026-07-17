// Blob URL helpers. Blobs (attachment bodies, inline cid images) are
// immutable and fetched by plain authenticated GET; the facade builds the
// URL with the token as a query parameter because anchors and <img> tags
// cannot set headers.

import type { BlobId } from '@/gen'
import { useMailClient } from './context'

/** Authenticated URL for a blob GET, for use in href/src attributes. */
export function useBlobUrl(): (blobId: BlobId) => string {
  const client = useMailClient()
  return (blobId) => client.blobUrl(blobId)
}

/** Authenticated URL for an account logo GET, for use in <img src>. */
export function useAccountLogoUrl(): (imageId: string) => string {
  const client = useMailClient()
  return (imageId) => client.accountLogoUrl(imageId)
}
