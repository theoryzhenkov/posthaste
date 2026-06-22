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
let streamStarting = false

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
    .openSession(sourceId === undefined ? {} : { sourceId })
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

function ensureStream(afterSeq?: number | null): void {
  if (unsubscribeStream || streamStarting) {
    return
  }
  streamStarting = true
  void ensureSession(activeSessionSourceId)
    .then((session) => {
      streamStarting = false
      if (frameHandlers.size === 0) {
        maybeCloseSession()
        return
      }
      unsubscribeStream = runtimeStream.subscribe(
        {
          sessionId: session.sessionId,
          afterSeq,
          ...(activeSessionSourceId === undefined
            ? {}
            : { sourceId: activeSessionSourceId }),
        },
        {
          onFrame(frame) {
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
            unsubscribeStream = undefined
            for (const handlers of frameHandlers) {
              handlers.onClosed?.(error)
            }
          },
        },
      )
    })
    .catch(() => {
      streamStarting = false
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
    options?: { afterSeq?: number | null; sourceId?: string | null },
  ): RuntimeUnsubscribe {
    frameHandlers.add(handlers)
    void ensureSession(options?.sourceId).then(() =>
      ensureStream(options?.afterSeq),
    )
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

  async runMutation(
    request: RuntimeSessionMutationRequest,
  ): Promise<RuntimeMutationReceipt> {
    const session = await ensureSession(request.sourceId)
    return runtimeStream.runMutation({
      sessionId: session.sessionId,
      name: request.name,
      args: request.args,
      clientMutationId:
        request.clientMutationId ?? randomRuntimeId('client_mutation'),
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
  streamStarting = false
  sessionPromise = undefined
  activeSession = undefined
  activeSessionSourceId = undefined
  frameHandlers.clear()
  openViewIds.clear()
}
