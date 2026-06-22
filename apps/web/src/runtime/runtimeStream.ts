import { getRuntimeAdapter } from './adapter'
import type {
  RuntimeFrameHandlers,
  RuntimeFrameSubscriptionRequest,
  RuntimeOpenSessionRequest,
  RuntimeOpenViewResult,
  RuntimeRunMutationRequest,
  RuntimeSessionObjectViewRequest,
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
  openView<TData = unknown>(request: RuntimeSessionObjectViewRequest) {
    return getRuntimeAdapter().openRuntimeSessionView(request) as Promise<
      RuntimeOpenViewResult<TData>
    >
  },
  closeView(sessionId: string, viewId: string, sourceId?: string | null) {
    return getRuntimeAdapter().closeRuntimeSessionView({
      sessionId,
      viewId,
      sourceId,
    })
  },
  runMutation(request: RuntimeRunMutationRequest) {
    return getRuntimeAdapter().runRuntimeMutation(request)
  },
  subscribe(
    request: RuntimeFrameSubscriptionRequest,
    handlers: RuntimeFrameHandlers,
  ): RuntimeUnsubscribe {
    return getRuntimeAdapter().subscribeRuntimeFrames(request, handlers)
  },
}
