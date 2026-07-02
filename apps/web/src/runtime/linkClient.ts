/**
 * The renderer's link-scoped facade over the runtime adapter.
 *
 * A THIN binding since M9b2 (D41): the transport policy that used to live here
 * — the reconnect timer, the `afterSeq` cursor threading, the retry loop — is
 * gone. The shared `LinkNearEnd` engine (wasm, behind the adapter's
 * link/stream/mutation methods) owns link open, reconnects, resume, and
 * retries; this module only multiplexes renderer consumers over one frame
 * subscription and reference-counts the link (open views + subscribers)
 * so it closes when nothing uses it.
 */
import { LOG_EVENTS, syncLogger } from '../logger'

import { runtimeStream } from './runtimeStream'
import type {
  RuntimeFrameHandlers,
  RuntimeMessagePageRequest,
  RuntimeMutationReceipt,
  RuntimeOpenMessageListViewResult,
  RuntimeOpenViewResult,
  RuntimeRunMutationRequest,
  RuntimeLinkConnection,
  RuntimeUnsubscribe,
} from './types'

type FrameHandlers = RuntimeFrameHandlers

type RuntimeLinkMutationRequest = Omit<
  RuntimeRunMutationRequest,
  'linkId' | 'clientMutationId'
> & {
  clientMutationId?: string | null
}

function randomRuntimeId(prefix: string): string {
  const random =
    typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
      ? crypto.randomUUID()
      : Math.random().toString(36).slice(2)
  return `${prefix}_${random}`
}

const frameHandlers = new Set<FrameHandlers>()
const openViewIds = new Set<string>()

let linkPromise: Promise<RuntimeLinkConnection> | undefined
let activeLink: RuntimeLinkConnection | undefined
let activeLinkSourceId: string | null | undefined
let unsubscribeStream: RuntimeUnsubscribe | undefined

function notifyPermanentError(error: unknown): void {
  for (const handlers of frameHandlers) {
    handlers.onPermanentError?.(error)
  }
}

function ensureLink(sourceId?: string | null): Promise<RuntimeLinkConnection> {
  if (linkPromise) {
    return linkPromise
  }
  activeLinkSourceId = sourceId
  linkPromise = runtimeStream
    // Opt into incremental mail-list deltas (replication client-link). Both client read
    // paths apply them: the default renderer reconciles directly, and the
    // replica adapter folds the delta into its served base.
    .openLink({
      ...(sourceId === undefined ? {} : { sourceId }),
      viewDelta: true,
    })
    .then((link) => {
      activeLink = link
      return link
    })
    .catch((error) => {
      linkPromise = undefined
      activeLink = undefined
      activeLinkSourceId = undefined
      notifyPermanentError(error)
      throw error
    })
  return linkPromise
}

/**
 * Ensure the one shared frame subscription exists. Reconnects, resume cursors,
 * and error classification are the engine's; a permanent engine stop surfaces
 * through each consumer's `onPermanentError`.
 */
function ensureStream(): void {
  if (unsubscribeStream) {
    return
  }
  void ensureLink(activeLinkSourceId).then((link) => {
    if (frameHandlers.size === 0) {
      maybeCloseLink()
      return
    }
    if (unsubscribeStream) {
      return
    }
    unsubscribeStream = runtimeStream.subscribe(
      {
        linkId: link.linkId,
        ...(activeLinkSourceId === undefined
          ? {}
          : { sourceId: activeLinkSourceId }),
      },
      {
        onFrame(frame) {
          syncLogger.debug(
            {
              event: LOG_EVENTS.runtimeFrameDispatched,
              linkId: link.linkId,
              type: frame.type,
              linkSeq: frame.linkSeq,
              ...(frame.type === 'viewReplace' || frame.type === 'viewSnapshot'
                ? { viewId: frame.viewId, revision: frame.revision }
                : {}),
            },
            'runtime frame dispatched',
          )
          for (const handlers of frameHandlers) {
            handlers.onFrame(frame)
          }
        },
        onMalformedFrame(input) {
          for (const handlers of frameHandlers) {
            handlers.onMalformedFrame?.(input)
          }
        },
        onPermanentError(error) {
          for (const handlers of frameHandlers) {
            handlers.onPermanentError?.(error)
          }
        },
        onTransientError(error) {
          for (const handlers of frameHandlers) {
            handlers.onTransientError?.(error)
          }
        },
        onClosed(error) {
          // A fake/test adapter may close its stream; the production engine
          // reconnects internally and never emits this.
          unsubscribeStream = undefined
          for (const handlers of frameHandlers) {
            handlers.onClosed?.(error)
          }
        },
      },
    )
  })
}

function maybeCloseLink(): void {
  if (frameHandlers.size > 0 || openViewIds.size > 0 || !activeLink) {
    return
  }
  const link = activeLink
  const sourceId = activeLinkSourceId
  unsubscribeStream?.()
  unsubscribeStream = undefined
  linkPromise = undefined
  activeLink = undefined
  activeLinkSourceId = undefined
  void runtimeStream.closeLink(link.linkId, sourceId).catch(() => {})
}

function sourceIdForView(request: RuntimeMessagePageRequest): string | null {
  return request.scope.kind === 'source-mailbox' ? request.scope.sourceId : null
}

function activeTransportSourceId(): string | null | undefined {
  return activeLinkSourceId === undefined ? undefined : activeLinkSourceId
}

export const runtimeLinkClient = {
  subscribe(
    handlers: FrameHandlers,
    options?: { sourceId?: string | null },
  ): RuntimeUnsubscribe {
    frameHandlers.add(handlers)
    void ensureLink(options?.sourceId).then(() => ensureStream())
    return () => {
      frameHandlers.delete(handlers)
      maybeCloseLink()
    }
  },

  async openMessageListView(
    request: RuntimeMessagePageRequest,
  ): Promise<RuntimeOpenMessageListViewResult> {
    const link = await ensureLink(sourceIdForView(request))
    const result = await runtimeStream.openMessageListView({
      linkId: link.linkId,
      view: request,
      sourceId: activeTransportSourceId(),
    })
    openViewIds.add(result.viewId)
    return result
  },

  async openView<TData = unknown>(request: {
    family: string
    payload: unknown
    sourceId?: string | null
  }): Promise<RuntimeOpenViewResult<TData>> {
    const link = await ensureLink(request.sourceId)
    const result = await runtimeStream.openView<TData>({
      linkId: link.linkId,
      descriptor: { family: request.family, payload: request.payload },
      sourceId: activeTransportSourceId(),
    })
    openViewIds.add(result.viewId)
    return result
  },

  async extendMessageListView(
    viewId: string,
    count: number,
  ): Promise<RuntimeOpenMessageListViewResult> {
    const link = await ensureLink(activeLinkSourceId)
    return runtimeStream.extendView({
      linkId: link.linkId,
      viewId,
      count,
      sourceId: activeTransportSourceId(),
    })
  },

  async runMutation(
    request: RuntimeLinkMutationRequest,
  ): Promise<RuntimeMutationReceipt> {
    const link = await ensureLink(request.sourceId)
    const clientMutationId =
      request.clientMutationId ?? randomRuntimeId('client_mutation')
    syncLogger.debug(
      {
        event: LOG_EVENTS.runtimeMutationSent,
        linkId: link.linkId,
        name: request.name,
        clientMutationId,
        sourceId: activeTransportSourceId(),
      },
      'runtime mutation sent',
    )
    return runtimeStream.runMutation({
      linkId: link.linkId,
      name: request.name,
      args: request.args,
      clientMutationId,
      context: request.context,
      sourceId: activeTransportSourceId(),
    })
  },

  closeView(viewId: string): void {
    if (!activeLink || !openViewIds.has(viewId)) {
      return
    }
    const link = activeLink
    const sourceId = activeLinkSourceId
    openViewIds.delete(viewId)
    void runtimeStream.closeView(link.linkId, viewId, sourceId).finally(() => {
      maybeCloseLink()
    })
  },
}

export function resetRuntimeLinkClientForTesting(): void {
  unsubscribeStream?.()
  unsubscribeStream = undefined
  linkPromise = undefined
  activeLink = undefined
  activeLinkSourceId = undefined
  frameHandlers.clear()
  openViewIds.clear()
}
