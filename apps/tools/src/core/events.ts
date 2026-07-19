// GET /events consumption under the level-triggered contract: every SSE
// message carries the current store generation; a dropped message heals at
// the next one, and reconnecting is the same code path as connecting. Event
// payloads are prompts — filters run client-side, and anything needing
// completeness reconciles through queries.

import type { EventMessage } from "@posthaste/protocol/gen";

import type { Connection } from "./connection.js";
import { ApiCallError, TransportError, parseErrorBody } from "./errors.js";

/** Open the SSE stream and return its byte stream; undefined when aborted. */
export async function openEventStream(
  conn: Connection,
  deps: { fetch?: typeof fetch; signal?: AbortSignal } = {},
): Promise<ReadableStream<Uint8Array> | undefined> {
  const doFetch = deps.fetch ?? conn.fetch ?? fetch;
  const headers: Record<string, string> = { accept: "text/event-stream" };
  if (conn.token) headers.authorization = `Bearer ${conn.token}`;

  let res: Response;
  try {
    res = await doFetch(`${conn.baseUrl}/events`, {
      method: "GET",
      headers,
      signal: deps.signal,
    });
  } catch (cause) {
    if (deps.signal?.aborted) return undefined;
    throw new TransportError(
      `event stream failed to open: ${cause instanceof Error ? cause.message : String(cause)}`,
    );
  }
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new ApiCallError(res.status, parseErrorBody(text), text || res.statusText);
  }
  // Coerce Bun's loosely-typed body to the standard byte-stream shape.
  return (res.body ?? undefined) as ReadableStream<Uint8Array> | undefined;
}

/**
 * Consume an SSE byte stream, awaiting `onData` per complete frame (frames
 * separated by a blank line). Awaiting gives serial back-pressure: `watch`
 * finishes handling one event (refetch + dispatch) before reading the next.
 */
export async function consumeSse(
  body: ReadableStream<Uint8Array>,
  onData: (data: string) => Promise<void> | void,
): Promise<void> {
  const decoder = new TextDecoder();
  const reader = body.getReader();
  let buffer = "";
  try {
    for (;;) {
      let chunk: Awaited<ReturnType<typeof reader.read>>;
      try {
        chunk = await reader.read();
      } catch {
        // Abort / connection drop mid-stream: the stream simply ends. The
        // level-triggered contract makes this safe — the caller reconnects
        // and the next message states current state.
        break;
      }
      if (chunk.done) break;
      buffer += decoder.decode(chunk.value, { stream: true });
      let boundary: number;
      while ((boundary = buffer.indexOf("\n\n")) >= 0) {
        const frame = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const data = frameData(frame);
        if (data !== undefined) await onData(data);
      }
    }
  } finally {
    reader.releaseLock();
  }
}

/** Join a frame's `data:` lines; undefined for comment-only frames. */
function frameData(frame: string): string | undefined {
  const dataLines: string[] = [];
  for (const line of frame.split("\n")) {
    if (line.startsWith(":")) continue; // comment / keep-alive
    if (line.startsWith("data:")) dataLines.push(line.slice(5).trimStart());
  }
  return dataLines.length > 0 ? dataLines.join("\n") : undefined;
}

/** Parse one SSE data payload into a typed [`EventMessage`], or undefined. */
export function parseEventMessage(data: string): EventMessage | undefined {
  let parsed: unknown;
  try {
    parsed = JSON.parse(data);
  } catch {
    return undefined;
  }
  if (
    typeof parsed !== "object" ||
    parsed === null ||
    typeof (parsed as EventMessage).generation !== "number"
  ) {
    return undefined;
  }
  return parsed as EventMessage;
}

/**
 * Open the stream and dispatch each parsed [`EventMessage`]. Returns when the
 * stream ends (server gone or `signal` aborted) — the caller decides whether
 * to reconnect; re-listening IS the recovery path, no cursor to replay.
 */
export async function forEachEvent(
  conn: Connection,
  onMessage: (message: EventMessage) => Promise<void> | void,
  deps: { fetch?: typeof fetch; signal?: AbortSignal } = {},
): Promise<void> {
  const body = await openEventStream(conn, deps);
  if (!body) return;
  await consumeSse(body, async (data) => {
    const message = parseEventMessage(data);
    if (message) await onMessage(message);
  });
}

/**
 * Resolve once the stream reports a generation >= `target` — the
 * read-your-writes helper: submit a command, await its generation, query.
 * Rejects on timeout (default 30s) or if the stream ends first.
 */
export async function awaitGenerationAtLeast(
  conn: Connection,
  target: number,
  deps: { fetch?: typeof fetch; signal?: AbortSignal; timeoutMs?: number } = {},
): Promise<number> {
  const controller = new AbortController();
  const abort = () => controller.abort();
  deps.signal?.addEventListener("abort", abort, { once: true });
  const timer = setTimeout(abort, deps.timeoutMs ?? 30_000);

  try {
    let seen: number | undefined;
    await forEachEvent(
      conn,
      (message) => {
        // First hit only: frames already buffered past the satisfying one
        // may still be delivered before the abort lands.
        if (seen === undefined && message.generation >= target) {
          seen = message.generation;
          controller.abort();
        }
      },
      { fetch: deps.fetch, signal: controller.signal },
    );
    if (seen === undefined) {
      throw new TransportError(
        `event stream ended before generation ${target} was observed`,
      );
    }
    return seen;
  } finally {
    clearTimeout(timer);
    deps.signal?.removeEventListener("abort", abort);
  }
}
