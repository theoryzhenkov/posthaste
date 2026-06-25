import { getRuntimeAdapter } from './adapter'
import type {
  RuntimeUnsubscribe,
  RuntimeViewFrameHandlers,
  RuntimeViewSubscriptionRequest,
} from './types'

export const runtimeSubscriptions = {
  view(
    request: RuntimeViewSubscriptionRequest,
    handlers: RuntimeViewFrameHandlers,
  ): RuntimeUnsubscribe {
    return getRuntimeAdapter().subscribeView(request, handlers)
  },
}
