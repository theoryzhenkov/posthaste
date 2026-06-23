import { getRuntimeAdapter } from './adapter'
import type {
  RuntimeResourceDescriptor,
  RuntimeResourceFetchOptions,
} from './types'

export const runtimeResources = {
  blob(
    descriptor: RuntimeResourceDescriptor,
    options?: RuntimeResourceFetchOptions,
  ): Promise<Blob> {
    return getRuntimeAdapter().fetchResourceBlob(descriptor, options)
  },
  /** Fetch a resource's bytes as text (e.g. the sanitized message body). */
  async text(
    descriptor: RuntimeResourceDescriptor,
    options?: RuntimeResourceFetchOptions,
  ): Promise<string> {
    const blob = await getRuntimeAdapter().fetchResourceBlob(
      descriptor,
      options,
    )
    return blob.text()
  },
}
