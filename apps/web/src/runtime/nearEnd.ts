/**
 * The client's link near-end, behind the wasm boundary (D41/M9b2).
 *
 * The shared `LinkNearEnd` engine (`posthaste-link-near-end`, compiled into
 * `posthaste-client-node-wasm`'s `NearEndHandle`) owns EVERY scrap of transport
 * policy the old TS fork held: link open, the reconnect loop, the
 * `afterSeq` resume cursor, request deadlines, jittered capped backoff,
 * typed frame parsing, permanent-vs-transient classification, and the
 * level-triggered reconciler (never-dispatched replay + the sent-but-unsettled
 * settlement query, D44).
 *
 * This module is the ZERO-POLICY host glue:
 *
 * - the browser IO shim — `fetch` and `fetchEventSource` wrapped as the
 *   engine's `postJson`/`getJson`/`openStream` callbacks (origin + auth
 *   headers only; no retries, no parsing, no cursor);
 * - the frame fan-out to the renderer's `RuntimeFrameHandlers`;
 * - the resume-cursor mirror to `sessionStorage` (the engine owns the cursor;
 *   the host only persists it across reloads);
 * - the pending-set-hook registry the entity-store adapter plugs its durable
 *   pending set into (the engine decides WHEN to reconcile; the adapter knows HOW
 *   to read/settle records).
 *
 * @spec docs/replication/client-link/L2
 */
import {
  EventStreamContentType,
  fetchEventSource,
} from '@microsoft/fetch-event-source'

import { authHeaders, baseUrl } from '../api/client'
import { LOG_EVENTS, syncLogger } from '../logger'

import { loadLinkWasmModule, type NearEndWasmHandle } from './replica/wasmUtil'
import type {
  RuntimeFrame,
  RuntimeFrameHandlers,
  RuntimeMailListViewState,
  RuntimeMutationReceipt,
  RuntimeRunMutationRequest,
  RuntimeUnsubscribe,
} from './types'

/** `sessionStorage` key for the engine's resume cursor (kept from the old
 * `useDaemonEvents` owner so a deploy does not lose the position). */
const CURSOR_STORAGE_KEY = 'mail:last-runtime-frame-seq'

// ---- the IO shim ------------------------------------------------------------

/** The transport surface handed to the engine — swappable in tests so suites
 * drive the REAL engine with fake IO instead of faking TS policy seams. */
export interface NearEndTransportIo {
  postJson(
    url: string,
    headersJson: string,
    body: string,
  ): Promise<{ status: number; body: string }>
  getJson(
    url: string,
    headersJson: string,
  ): Promise<{ status: number; body: string }>
  /** Open a frame stream; report via `onEvent(kind, data, status)` where kind
   * is one of `open|message|closed|error` (status `-1` = none). Returns an
   * abort function. MUST NOT retry — the engine owns reconnects. */
  openStream(
    url: string,
    onEvent: (kind: string, data: string, status: number) => void,
  ): () => void
}

function parseHeaders(headersJson: string): Record<string, string> {
  try {
    const entries = JSON.parse(headersJson) as [string, string][]
    return Object.fromEntries(entries)
  } catch {
    return {}
  }
}

async function fetchJson(
  method: 'POST' | 'GET',
  url: string,
  headersJson: string,
  body?: string,
): Promise<{ status: number; body: string }> {
  const response = await fetch(`${baseUrl()}${url}`, {
    method,
    headers: { ...authHeaders(), ...parseHeaders(headersJson) },
    ...(body ? { body } : {}),
  })
  return { status: response.status, body: await response.text() }
}

/** A sentinel that stops `fetchEventSource`'s internal retry machinery: the
 * engine owns every reconnect decision, so the shim always terminates. */
class StreamHalt extends Error {}

const browserTransportIo: NearEndTransportIo = {
  postJson: (url, headersJson, body) =>
    fetchJson('POST', url, headersJson, body),
  getJson: (url, headersJson) => fetchJson('GET', url, headersJson),
  openStream(url, onEvent) {
    const controller = new AbortController()
    void fetchEventSource(`${baseUrl()}${url}`, {
      headers: authHeaders(),
      signal: controller.signal,
      openWhenHidden: true,
      async onopen(response) {
        const contentType = response.headers.get('content-type') ?? ''
        if (response.ok && contentType.startsWith(EventStreamContentType)) {
          onEvent('open', '', -1)
          return
        }
        onEvent(
          'error',
          `runtime stream rejected with ${response.status}`,
          response.status,
        )
        throw new StreamHalt()
      },
      onmessage(event) {
        onEvent('message', event.data ?? '', -1)
      },
      onclose() {
        onEvent('closed', '', -1)
        throw new StreamHalt()
      },
      onerror(error) {
        if (!(error instanceof StreamHalt) && !controller.signal.aborted) {
          onEvent('error', String(error), -1)
        }
        // Rethrow: never let fetchEventSource retry on its own.
        throw error
      },
    }).catch(() => {
      // Terminations are already reported through onEvent above.
    })
    return () => controller.abort()
  },
}

let transportIo: NearEndTransportIo = browserTransportIo

// ---- pending-set hooks --------------------------------------------------------

/** A sent-but-unsettled record for the engine's settlement query (D44b). */
export interface NearEndSentUnsettled {
  /** The link the record was dispatched under. */
  linkId: string
  clientMutationId: string
  request?: RuntimeRunMutationRequest
}

/** The durable-pending-set surface the engine's level-triggered reconciler drives.
 * Registered by the entity-store adapter; defaults are inert. */
export interface NearEndPendingSetHooks {
  /** Requests accepted optimistically with no evidence they reached the
   * runtime — replayed on every connect. */
  neverDispatched(): Promise<RuntimeRunMutationRequest[]>
  /** A replayed forward succeeded: link the receipt so the record is no longer
   * never-dispatched. `linkId` is the link it was re-sent under. */
  onReconciled(
    receipt: RuntimeMutationReceipt,
    linkId: string | null,
  ): Promise<void>
  /** Records with a receipt but no terminal settlement (link-continuity
   * loss) — queried against the runtime on every connect. */
  sentUnsettled(): Promise<NearEndSentUnsettled[]>
  /** The settlement query found a terminal verdict: settle locally. */
  onSettlement(receipt: RuntimeMutationReceipt): Promise<void>
}

const inertPendingSetHooks: NearEndPendingSetHooks = {
  neverDispatched: async () => [],
  onReconciled: async () => {},
  sentUnsettled: async () => [],
  onSettlement: async () => {},
}

let pendingSetHooks: NearEndPendingSetHooks = inertPendingSetHooks

/** Plug the durable pending set into the engine's reconciler (the entity-store
 * adapter calls this once at construction). */
export function setNearEndPendingSetHooks(hooks: NearEndPendingSetHooks): void {
  pendingSetHooks = hooks
}

// ---- engine lifecycle -----------------------------------------------------------

const frameHandlers = new Set<RuntimeFrameHandlers>()

let engine: NearEndWasmHandle | null = null
let engineSourceId: string | null | undefined
let connectPromise: Promise<{ linkId: string }> | undefined

/** The wire shape the engine's `MutationRequest` parse accepts — strip the
 * TS-side extras (`sourceId` travels as engine config, never in the body). */
function wireMutationRequest(request: RuntimeRunMutationRequest): {
  linkId?: string | null
  name: string
  args?: unknown
  clientMutationId: string
  context?: unknown
} {
  return {
    name: request.name,
    args: request.args,
    clientMutationId: request.clientMutationId,
    ...(request.context !== undefined ? { context: request.context } : {}),
  }
}

function storedCursor(): number | undefined {
  try {
    const stored = window.sessionStorage.getItem(CURSOR_STORAGE_KEY)
    const parsed = stored ? Number.parseInt(stored, 10) : Number.NaN
    return Number.isFinite(parsed) ? parsed : undefined
  } catch {
    return undefined
  }
}

function persistCursor(linkSeq: number): void {
  try {
    window.sessionStorage.setItem(CURSOR_STORAGE_KEY, String(linkSeq))
  } catch {
    // Storage unavailable: resume starts fresh next load.
  }
}

function buildIo() {
  return {
    postJson: (url: string, headersJson: string, body: string) =>
      transportIo.postJson(url, headersJson, body),
    getJson: (url: string, headersJson: string) =>
      transportIo.getJson(url, headersJson),
    openStream: (
      url: string,
      onEvent: (kind: string, data: string, status: number) => void,
    ) => transportIo.openStream(url, onEvent),
    onFrame(json: string) {
      // The engine already parsed + validated the frame; this parse only
      // rehydrates it across the JSON-string boundary.
      const frame = JSON.parse(json) as RuntimeFrame<RuntimeMailListViewState>
      persistCursor(frame.linkSeq)
      for (const handlers of frameHandlers) {
        handlers.onFrame(frame)
      }
    },
    onMalformed(raw: string, error: string) {
      for (const handlers of frameHandlers) {
        handlers.onMalformedFrame?.({ raw, error })
      }
    },
    onReset() {
      // D49: the near node's incremental view is broken (a seq gap the far-end
      // could not replay). Surface it so the adapter re-seeds; the runtime then
      // re-serves whole snapshots over the fresh subscription.
      for (const handlers of frameHandlers) {
        handlers.onReset?.()
      }
    },
    onLinkReestablished(linkId: string) {
      // M44/D112 recovery edge: the engine re-prepared a FRESH link (new id)
      // after the prior one was reaped/dropped. Fan it out so the host adopts
      // the id + reconciles its server-served views/caches (RC1/RC2/RC3).
      syncLogger.debug(
        { event: LOG_EVENTS.runtimeAdapterInitialized, linkId },
        'near-end link re-established (recovery edge)',
      )
      for (const handlers of frameHandlers) {
        handlers.onLinkReestablished?.(linkId)
      }
    },
    onStatus(label: string, message: string) {
      switch (label) {
        case 'permanentError':
        case 'degraded':
          // 'degraded' ([3]): N consecutive malformed frames — a version skew /
          // corrupt peer. The engine has stopped, so surface it as a permanent
          // error the host must re-establish from.
          for (const handlers of frameHandlers) {
            handlers.onPermanentError?.(new Error(message))
          }
          break
        case 'transientError':
          for (const handlers of frameHandlers) {
            handlers.onTransientError?.(new Error(message))
          }
          break
        default:
          // connecting/connected/reconnecting: engine-internal lifecycle.
          break
      }
    },
    neverDispatched: async () => {
      const requests = await pendingSetHooks.neverDispatched()
      return JSON.stringify(requests.map(wireMutationRequest))
    },
    onReconciled(receiptJson: string) {
      const receipt = JSON.parse(receiptJson) as RuntimeMutationReceipt
      void pendingSetHooks.onReconciled(receipt, engine?.linkId() ?? null)
    },
    sentUnsettled: async () => {
      const records = await pendingSetHooks.sentUnsettled()
      return JSON.stringify(
        records.map((record) => ({
          linkId: record.linkId,
          clientMutationId: record.clientMutationId,
          ...(record.request
            ? { request: wireMutationRequest(record.request) }
            : {}),
        })),
      )
    },
    onSettlement(receiptJson: string) {
      const receipt = JSON.parse(receiptJson) as RuntimeMutationReceipt
      void pendingSetHooks.onSettlement(receipt)
    },
  }
}

async function createEngine(
  sourceId: string | null | undefined,
): Promise<NearEndWasmHandle> {
  const module = await loadLinkWasmModule()
  const config = {
    viewDelta: true,
    ...(sourceId ? { sourceId } : {}),
    ...(storedCursor() !== undefined ? { initialCursor: storedCursor() } : {}),
  }
  return new module.NearEndHandle(buildIo(), JSON.stringify(config))
}

/**
 * Ensure the engine exists for `sourceId` and its link is open; resolves
 * with the link id. Reuses the live engine while the source scope matches;
 * a scope change tears the old engine down first.
 */
export function connectNearEnd(options?: {
  sourceId?: string | null
}): Promise<{ linkId: string }> {
  const sourceId = options?.sourceId
  if (connectPromise && engineSourceId === sourceId) {
    return connectPromise
  }
  if (connectPromise && engineSourceId !== sourceId) {
    void disconnectNearEnd()
  }
  engineSourceId = sourceId
  connectPromise = (async () => {
    const handle = await createEngine(sourceId)
    engine = handle
    await handle.connect()
    const linkId = handle.linkId()
    if (!linkId) {
      throw new Error('near-end engine connected without a link id')
    }
    syncLogger.debug(
      { event: LOG_EVENTS.runtimeAdapterInitialized, linkId },
      'near-end engine connected',
    )
    return { linkId }
  })()
  connectPromise.catch(() => {
    // A failed connect is not sticky: the next call retries from scratch.
    connectPromise = undefined
    engine = null
  })
  return connectPromise
}

/** Stop the frame loop (no further reconnects) and drop the engine. Link
 * close on the server is the caller's concern (a policy-free DELETE). */
export async function disconnectNearEnd(): Promise<void> {
  const handle = engine
  engine = null
  connectPromise = undefined
  engineSourceId = undefined
  if (handle) {
    await handle.disconnect()
    handle.free()
  }
}

/** The engine's current link id, once connected. */
export function nearEndLinkId(): string | null {
  return engine?.linkId() ?? null
}

/**
 * Register a renderer frame consumer. The engine (and its reconnect loop) is
 * started on first use; handlers multiplex over the one engine stream.
 */
export function subscribeNearEndFrames(
  handlers: RuntimeFrameHandlers,
): RuntimeUnsubscribe {
  frameHandlers.add(handlers)
  return () => {
    frameHandlers.delete(handlers)
  }
}

/**
 * Forward a mutation through the engine: deadline, jittered transient retry,
 * typed receipt parse — all engine policy. Ensures the link first.
 */
export async function forwardNearEndMutation(
  request: RuntimeRunMutationRequest,
): Promise<RuntimeMutationReceipt> {
  await connectNearEnd({ sourceId: request.sourceId })
  const handle = engine
  if (!handle) {
    throw new Error('near-end engine is not connected')
  }
  const receiptJson = await handle.forward(
    JSON.stringify(wireMutationRequest(request)),
  )
  return JSON.parse(receiptJson) as RuntimeMutationReceipt
}

// ---- test seams ------------------------------------------------------------------

/** Swap the transport IO under the REAL engine (fake `postJson`/`openStream`
 * callbacks) — how the adapter-seam suites drive engine behavior. */
export function setNearEndTransportIoForTesting(io: NearEndTransportIo): void {
  transportIo = io
}

/** Tear the engine + registries back to their initial state. */
export async function resetNearEndForTesting(): Promise<void> {
  await disconnectNearEnd()
  frameHandlers.clear()
  pendingSetHooks = inertPendingSetHooks
  transportIo = browserTransportIo
}
