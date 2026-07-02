/**
 * The renderer's session-scoped facade over the runtime adapter.
 *
 * A THIN binding since M9b2 (D41): the transport policy that used to live here
 * — the reconnect timer, the `afterSeq` cursor threading, the retry loop — is
 * gone. The shared `LinkNearEnd` engine (wasm, behind the adapter's
 * session/stream/mutation methods) owns session open, reconnects, resume, and
 * retries; this module only multiplexes renderer consumers over one frame
 * subscription and reference-counts the session (open views + subscribers)
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
  RuntimeSession,
  RuntimeUnsubscribe,
} from './types'

type FrameHandlers = RuntimeFrameHandlers

type RuntimeSessionMutationRequest = Omit<
  RuntimeRunMutationRequest,
  'sessionId' | 'clientMutationId'
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

let sessionPromise: Promise<RuntimeSession> | undefined
let activeSession: RuntimeSession | undefined
let activeSessionSourceId: string | null | undefined
let unsubscribeStream: RuntimeUnsubscribe | undefined

function notifyPermanentError(error: unknown): void {
  for (const handlers of frameHandlers) {
    handlers.onPermanentError?.(error)
  }
}

function ensureSession(sourceId?: string | null): Promise<RuntimeSession> {
  if (sessionPromise) {
    return sessionPromise
  }
  activeSessionSourceId = sourceId
  sessionPromise = runtimeStream
    // Opt into incremental mail-list deltas (replication client-link). Both client read
    // paths apply them: the default renderer reconciles directly, and the
    // replica adapter folds the delta into its served base.
    .openSession({
      ...(sourceId === undefined ? {} : { sourceId }),
      viewDelta: true,
    })
    .then((session) => {
      activeSession = session
      return session
    })
    .catch((error) => {
      sessionPromise = undefined
      activeSession = undefined
      activeSessionSourceId = undefined
      notifyPermanentError(error)
      throw error
    })
  return sessionPromise
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
  void ensureSession(activeSessionSourceId).then((session) => {
    if (frameHandlers.size === 0) {
      maybeCloseSession()
      return
    }
    if (unsubscribeStream) {
      return
    }
    unsubscribeStream = runtimeStream.subscribe(
      {
        sessionId: session.sessionId,
        ...(activeSessionSourceId === undefined
          ? {}
          : { sourceId: activeSessionSourceId }),
      },
      {
        onFrame(frame) {
          syncLogger.debug(
            {
              event: LOG_EVENTS.runtimeFrameDispatched,
              sessionId: session.sessionId,
              type: frame.type,
              sessionSeq: frame.sessionSeq,
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

function maybeCloseSession(): void {
  if (frameHandlers.size > 0 || openViewIds.size > 0 || !activeSession) {
    return
  }
  const session = activeSession
  const sourceId = activeSessionSourceId
  unsubscribeStream?.()
  unsubscribeStream = undefined
  sessionPromise = undefined
  activeSession = undefined
  activeSessionSourceId = undefined
  void runtimeStream.closeSession(session.sessionId, sourceId).catch(() => {})
}

function sourceIdForView(request: RuntimeMessagePageRequest): string | null {
  return request.scope.kind === 'source-mailbox' ? request.scope.sourceId : null
}

function activeTransportSourceId(): string | null | undefined {
  return activeSessionSourceId === undefined ? undefined : activeSessionSourceId
}

export const runtimeSessionClient = {
  subscribe(
    handlers: FrameHandlers,
    options?: { sourceId?: string | null },
  ): RuntimeUnsubscribe {
    frameHandlers.add(handlers)
    void ensureSession(options?.sourceId).then(() => ensureStream())
    return () => {
      frameHandlers.delete(handlers)
      maybeCloseSession()
    }
  },

  async openMessageListView(
    request: RuntimeMessagePageRequest,
  ): Promise<RuntimeOpenMessageListViewResult> {
    const session = await ensureSession(sourceIdForView(request))
    const result = await runtimeStream.openMessageListView({
      sessionId: session.sessionId,
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
    const session = await ensureSession(request.sourceId)
    const result = await runtimeStream.openView<TData>({
      sessionId: session.sessionId,
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
    const session = await ensureSession(activeSessionSourceId)
    return runtimeStream.extendView({
      sessionId: session.sessionId,
      viewId,
      count,
      sourceId: activeTransportSourceId(),
    })
  },

  async runMutation(
    request: RuntimeSessionMutationRequest,
  ): Promise<RuntimeMutationReceipt> {
    const session = await ensureSession(request.sourceId)
    const clientMutationId =
      request.clientMutationId ?? randomRuntimeId('client_mutation')
    syncLogger.debug(
      {
        event: LOG_EVENTS.runtimeMutationSent,
        sessionId: session.sessionId,
        name: request.name,
        clientMutationId,
        sourceId: activeTransportSourceId(),
      },
      'runtime mutation sent',
    )
    return runtimeStream.runMutation({
      sessionId: session.sessionId,
      name: request.name,
      args: request.args,
      clientMutationId,
      context: request.context,
      sourceId: activeTransportSourceId(),
    })
  },

  closeView(viewId: string): void {
    if (!activeSession || !openViewIds.has(viewId)) {
      return
    }
    const session = activeSession
    const sourceId = activeSessionSourceId
    openViewIds.delete(viewId)
    void runtimeStream
      .closeView(session.sessionId, viewId, sourceId)
      .finally(() => {
        maybeCloseSession()
      })
  },
}

export function resetRuntimeSessionClientForTesting(): void {
  unsubscribeStream?.()
  unsubscribeStream = undefined
  sessionPromise = undefined
  activeSession = undefined
  activeSessionSourceId = undefined
  frameHandlers.clear()
  openViewIds.clear()
}
