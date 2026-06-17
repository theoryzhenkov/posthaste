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
}
