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
import { setConnectionHealth } from '@/live-store/store'

import {
  __setRuntimeAdapterReadyGateForTesting,
  whenRuntimeAdapterReady,
} from './adapter'
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

/**
 * Server-served views register a re-open callback here (the accountStatus view,
 * mail-list views, message detail/conversation). On the M44 recovery edge the
 * reconcile walks this registry so every open view re-serves its base against
 * the FRESH link — the per-view re-open shape (RC1), which fits the existing
 * hook lifecycle better than a central request registry (the hook already owns
 * its open/close; re-open is one more effect trigger it drives).
 */
const reopenHandlers = new Set<() => void>()

let linkPromise: Promise<RuntimeLinkConnection> | undefined
let activeLink: RuntimeLinkConnection | undefined
let activeLinkSourceId: string | null | undefined
let unsubscribeStream: RuntimeUnsubscribe | undefined
/** Tracks a hidden→visible transition so the reconcile fires only on a genuine
 *  tab foreground (a hidden tab's link may have been idle-reaped). */
let wasHidden = false

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
  // CL-C2 / R1: gate the first link open on the entity-store install so this
  // link's frame subscription + view-opens bind to the entity-store adapter, not
  // the transient base adapter that would strand the session (no ingest, counts,
  // or synthesized viewReplace) until a reload. Bounded inside the gate, so a
  // stuck install still lets the link open (degraded) rather than hang.
  linkPromise = whenRuntimeAdapterReady()
    .then(() =>
      // Opt into incremental mail-list deltas (replication client-link). Both
      // client read paths apply them: the default renderer reconciles directly,
      // and the replica adapter folds the delta into its served base.
      runtimeStream.openLink({
        ...(sourceId === undefined ? {} : { sourceId }),
        viewDelta: true,
      }),
    )
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
        onLinkReestablished(newLinkId) {
          handleLinkReestablished(newLinkId)
        },
      },
    )
  })
}

/**
 * The M44 recovery-edge reconcile (D112). The near-end engine re-prepared a
 * FRESH link (new id) after the prior one was idle-reaped or dropped. This is
 * the client twin of the server's level-triggered reconciler — it converges the
 * client to the fresh link from any state, with no reload:
 *
 * - RC3: adopt the fresh link id so subsequent open/extend/close stop 404-ing
 *   against the dead link (the old id was pinned at first connect).
 * - RC1/RC2: re-drive every open server-served view against the fresh link so
 *   the server re-serves their base frames (mail lists, the accountStatus view
 *   over `queryKeys.accounts`, message detail/conversation) — this replays the
 *   frames lost in the re-prepare gap, including the terminal sync-Ready
 *   `account.status_changed` that clears an empty mailbox's `isSyncing`.
 */
function handleLinkReestablished(newLinkId: string): void {
  if (activeLink && activeLink.linkId !== newLinkId) {
    // RC3: adopt the fresh id in place so every holder of the resolved link
    // connection (open/extend/close, all of which read `activeLink.linkId`)
    // targets the live link.
    activeLink.linkId = newLinkId
  }
  syncLogger.debug(
    { event: LOG_EVENTS.runtimeAdapterInitialized, linkId: newLinkId },
    'link client adopted re-established link; reconciling open views',
  )
  reconcileOpenViews()
}

/** Re-serve every open server-served view (RC1) and blip connection health.
 *  Shared by the engine recovery edge and the tab-foreground edge. */
function reconcileOpenViews(): void {
  setConnectionHealth('recovering')
  for (const reopen of [...reopenHandlers]) {
    try {
      reopen()
    } catch (error) {
      // A single view's re-open failure must not strand the others.
      syncLogger.warn(
        {
          event: LOG_EVENTS.runtimeAdapterInitialized,
          error: error instanceof Error ? error.message : String(error),
        },
        'a view re-open failed during the recovery reconcile',
      )
    }
  }
  // The re-opens have been dispatched; return to healthy on the next microtask
  // so a synchronous health subscriber still observes the 'recovering' blip.
  queueMicrotask(() => setConnectionHealth('healthy'))
}

function onVisibilityChange(): void {
  if (typeof document === 'undefined') {
    return
  }
  if (document.visibilityState === 'hidden') {
    wasHidden = true
    return
  }
  if (document.visibilityState === 'visible' && wasHidden) {
    wasHidden = false
    // A hidden tab's link may have been idle-reaped while the engine's stream
    // was paused; re-serve open views defensively (level-triggered — safe even
    // when the link is still valid). Reuses the M31 visibility wiring shape.
    if (activeLink) {
      reconcileOpenViews()
    }
  }
}

let visibilityHookInstalled = false

/** Install the tab-foreground reconcile hook once (no-op outside a DOM). */
function installLinkRecoveryVisibilityHook(): void {
  if (visibilityHookInstalled || typeof document === 'undefined') {
    return
  }
  document.addEventListener('visibilitychange', onVisibilityChange)
  visibilityHookInstalled = true
}

installLinkRecoveryVisibilityHook()

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

  /**
   * Register a server-served view's re-open callback for the M44 recovery edge
   * (RC1). The callback is invoked when the near-end engine re-prepares a fresh
   * link (or a hidden tab is foregrounded), by which time `activeLink.linkId` is
   * already the fresh id — so the callback re-opening its view targets the live
   * link. Returns an unsubscribe. A view owner (a hook) typically re-opens by
   * bumping the effect that owns its open/close lifecycle.
   */
  onLinkReestablished(callback: () => void): RuntimeUnsubscribe {
    reopenHandlers.add(callback)
    return () => {
      reopenHandlers.delete(callback)
    }
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
  reopenHandlers.clear()
  wasHidden = false
  // Tests wire the adapter synchronously via `setRuntimeAdapterForTesting`, so
  // the install gate is considered satisfied; clear it back to resolved (a test
  // modelling the CL-C2 race pins its own pending gate AFTER this reset).
  __setRuntimeAdapterReadyGateForTesting(Promise.resolve())
}
