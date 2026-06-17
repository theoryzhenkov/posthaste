import { getRuntimeAdapter } from './adapter'
import type {
  RuntimeEventHandlers,
  RuntimeEventSubscriptionRequest,
  RuntimeUnsubscribe,
} from './types'

export const runtimeSubscriptions = {
  events(
    request: RuntimeEventSubscriptionRequest,
    handlers: RuntimeEventHandlers,
  ): RuntimeUnsubscribe {
    return getRuntimeAdapter().subscribeEvents(request, handlers)
  },
}
