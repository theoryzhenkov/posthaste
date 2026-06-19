import { getRuntimeAdapter } from './adapter'
import type {
  RuntimeEventHandlers,
  RuntimeEventSubscriptionRequest,
  RuntimeUnsubscribe,
  RuntimeViewFrameHandlers,
  RuntimeViewSubscriptionRequest,
} from './types'

export const runtimeSubscriptions = {
  events(
    request: RuntimeEventSubscriptionRequest,
    handlers: RuntimeEventHandlers,
  ): RuntimeUnsubscribe {
    return getRuntimeAdapter().subscribeEvents(request, handlers)
  },
  view(
    request: RuntimeViewSubscriptionRequest,
    handlers: RuntimeViewFrameHandlers,
  ): RuntimeUnsubscribe {
    return getRuntimeAdapter().subscribeView(request, handlers)
  },
}
