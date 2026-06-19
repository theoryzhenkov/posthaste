import { getRuntimeAdapter } from './adapter'
import type {
  RuntimeFrameHandlers,
  RuntimeFrameSubscriptionRequest,
  RuntimeOpenSessionRequest,
  RuntimeSessionViewRequest,
  RuntimeUnsubscribe,
} from './types'

export const runtimeStream = {
  openSession(request: RuntimeOpenSessionRequest) {
    return getRuntimeAdapter().openRuntimeSession(request)
  },
  closeSession(sessionId: string, sourceId?: string | null) {
    return getRuntimeAdapter().closeRuntimeSession({ sessionId, sourceId })
  },
  openMessageListView(request: RuntimeSessionViewRequest) {
    return getRuntimeAdapter().openRuntimeSessionMessageListView(request)
  },
  subscribe(
    request: RuntimeFrameSubscriptionRequest,
    handlers: RuntimeFrameHandlers,
  ): RuntimeUnsubscribe {
    return getRuntimeAdapter().subscribeRuntimeFrames(request, handlers)
  },
}
