import { getRuntimeAdapter } from './adapter'
import type {
  RuntimeFrameHandlers,
  RuntimeFrameSubscriptionRequest,
  RuntimeOpenLinkRequest,
  RuntimeOpenViewResult,
  RuntimeRunMutationRequest,
  RuntimeLinkObjectViewRequest,
  RuntimeLinkViewExtendRequest,
  RuntimeLinkViewRequest,
  RuntimeUnsubscribe,
} from './types'

export const runtimeStream = {
  openLink(request: RuntimeOpenLinkRequest) {
    return getRuntimeAdapter().openRuntimeLink(request)
  },
  closeLink(linkId: string, sourceId?: string | null) {
    return getRuntimeAdapter().closeRuntimeLink({ linkId, sourceId })
  },
  openMessageListView(request: RuntimeLinkViewRequest) {
    return getRuntimeAdapter().openRuntimeLinkMessageListView(request)
  },
  openView<TData = unknown>(request: RuntimeLinkObjectViewRequest) {
    return getRuntimeAdapter().openRuntimeLinkView(request) as Promise<
      RuntimeOpenViewResult<TData>
    >
  },
  extendView(request: RuntimeLinkViewExtendRequest) {
    return getRuntimeAdapter().extendRuntimeLinkView(request)
  },
  closeView(linkId: string, viewId: string, sourceId?: string | null) {
    return getRuntimeAdapter().closeRuntimeLinkView({
      linkId,
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
