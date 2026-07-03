import { ApiError, type Connection } from "../client.js";

/** Filters for the domain-event stream (mirror of the `EventFilter` query). */
export interface EventsOptions {
  afterSeq?: number;
  topic?: string;
  account?: string;
  mailbox?: string;
}

/** Build the `GET /v1/events` URL with the active filters applied. */
function eventsUrl(conn: Connection, opts: EventsOptions): URL {
  const url = new URL(conn.baseUrl + "/events");
  if (opts.account) url.searchParams.set("accountId", opts.account);
  if (opts.topic) url.searchParams.set("topic", opts.topic);
  if (opts.mailbox) url.searchParams.set("mailboxId", opts.mailbox);
  if (opts.afterSeq !== undefined) {
    url.searchParams.set("afterSeq", String(opts.afterSeq));
  }
  return url;
}

/**
 * Open the daemon's `GET /v1/events` SSE and return the response byte stream
 * (or `undefined` if the request was aborted or has no body). Throws an
 * [`ApiError`] on a non-2xx open or a transport failure. Shared by the `events`
 * tap and the `watch` runner.
 */
export async function openEventStream(
  conn: Connection,
  opts: EventsOptions,
  deps: { fetch: typeof fetch; signal?: AbortSignal },
): Promise<ReadableStream<Uint8Array> | undefined> {
  const url = eventsUrl(conn, opts);
  const headers: Record<string, string> = { accept: "text/event-stream" };
  if (conn.token) headers.authorization = `Bearer ${conn.token}`;

  let res: Response;
  try {
    res = await deps.fetch(url, {
      method: "GET",
      headers,
      signal: deps.signal,
    });
  } catch (cause) {
    if (deps.signal?.aborted) return undefined;
    throw new ApiError(
      0,
      undefined,
      `event stream to ${url.toString()} failed: ${cause instanceof Error ? cause.message : String(cause)}`,
    );
  }

  if (!res.ok) {
    const text = await res.text().catch(() => "");
    let code: string | undefined;
    let message = text || res.statusText;
    try {
      const body = text ? JSON.parse(text) : undefined;
      code = body?.code;
      message = body?.message ?? message;
    } catch {
      /* non-JSON body; keep the raw text */
    }
    const hint =
      res.status === 404
        ? " (the daemon may not expose GET /v1/events — see posthastectl docs)"
        : "";
    throw new ApiError(
      res.status,
      code,
      `event stream ${res.status}${code ? ` [${code}]` : ""}: ${message}${hint}`,
    );
  }

  return res.body ?? undefined;
}

/**
 * One parsed SSE frame: the joined `data:` payload, the optional `id:`, and the
 * optional `event:` type (absent → the default `message` type).
 */
interface SseFrame {
  data: string;
  id?: string;
  event?: string;
}

/** Parse a single SSE frame into its data payload + id + event type (comments ignored). */
function parseFrame(frame: string): SseFrame | undefined {
  const dataLines: string[] = [];
  let id: string | undefined;
  let event: string | undefined;
  for (const line of frame.split("\n")) {
    if (line.startsWith(":")) continue; // comment / keep-alive
    if (line.startsWith("data:")) dataLines.push(line.slice(5).trimStart());
    else if (line.startsWith("id:")) id = line.slice(3).trim();
    else if (line.startsWith("event:")) event = line.slice(6).trim();
  }
  if (dataLines.length === 0) return undefined;
  return { data: dataLines.join("\n"), id, event };
}

/**
 * Decide whether a frame is a **gap frame** — the tap's signal that the resume
 * cursor fell before the durable log's oldest retained seq, so the missed range
 * is gone and cannot be replayed. A frame is a gap when its SSE `event:` type is
 * `"gap"` or its JSON `data` carries `kind === "reset"`. Returns the log's
 * current `highestSeq` (the position the consumer must adopt), or `undefined`
 * for an ordinary `DomainEvent` frame. Parses defensively: a non-JSON or
 * seq-less gap payload still yields a gap with `highestSeq` of `0`.
 */
export function gapFrame(
  data: string,
  event?: string,
): { highestSeq: number } | undefined {
  let parsed: unknown;
  try {
    parsed = JSON.parse(data);
  } catch {
    parsed = undefined;
  }
  const isReset =
    typeof parsed === "object" &&
    parsed !== null &&
    (parsed as { kind?: unknown }).kind === "reset";
  if (event !== "gap" && !isReset) return undefined;
  const raw =
    typeof parsed === "object" && parsed !== null
      ? (parsed as { highestSeq?: unknown }).highestSeq
      : undefined;
  const highestSeq = typeof raw === "number" && Number.isFinite(raw) ? raw : 0;
  return { highestSeq };
}

/**
 * Consume an SSE byte stream, **awaiting** `onData` for each complete event
 * (frames separated by a blank line). Awaiting gives serial back-pressure: the
 * next frame is not read until the current one is fully handled — exactly what
 * `watch` needs so one message is processed (fetched + dispatched) at a time.
 */
export async function consumeSse(
  body: ReadableStream<Uint8Array>,
  onData: (data: string, id?: string, event?: string) => Promise<void> | void,
): Promise<void> {
  const decoder = new TextDecoder();
  const reader = body.getReader();
  let buffer = "";

  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });

      let boundary: number;
      while ((boundary = buffer.indexOf("\n\n")) >= 0) {
        const frame = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const parsed = parseFrame(frame);
        if (parsed) await onData(parsed.data, parsed.id, parsed.event);
      }
    }
  } finally {
    reader.releaseLock();
  }
}

/**
 * Callbacks over a live domain-event stream: one for an ordinary `DomainEvent`
 * frame, one for a **gap frame** (the tap's durability signal). Both the CLI
 * `events` tap and the MCP subscription are built on these — the SSE
 * open/parse/gap-detect primitives (`openEventStream`/`consumeSse`/`gapFrame`)
 * live in one place so a second consumer never re-derives the framing.
 */
export interface EventStreamHandlers {
  /** An ordinary event: the raw JSON `data` payload and the SSE `id:` (the seq). */
  onEvent: (data: string, seq?: string) => Promise<void> | void;
  /** A gap: the log's current `highestSeq` (adopt as the new cursor) + raw payload. */
  onGap: (highestSeq: number, data: string) => Promise<void> | void;
}

/**
 * Open `GET /v1/events` and dispatch each parsed frame to `handlers`, routing
 * gap frames to `onGap` and everything else to `onEvent`. The single place the
 * SSE stream is opened and demultiplexed; callers supply only what to *do* with
 * a frame, never how to parse one.
 */
export async function forwardEventStream(
  conn: Connection,
  opts: EventsOptions,
  deps: { fetch: typeof fetch; signal?: AbortSignal },
  handlers: EventStreamHandlers,
): Promise<void> {
  const body = await openEventStream(conn, opts, deps);
  if (!body) return;
  await consumeSse(body, async (data, id, event) => {
    const gap = gapFrame(data, event);
    if (gap) {
      await handlers.onGap(gap.highestSeq, data);
      return;
    }
    await handlers.onEvent(data, id);
  });
}

/** Injectable side-effects for the `events` tap. */
export interface EventsDeps {
  fetch: typeof fetch;
  /** Emit one event as a single NDJSON line (the newline is added here). */
  emit: (line: string) => void;
  /** Diagnostics (e.g. the resolved last seq). */
  log?: (line: string) => void;
  signal?: AbortSignal;
}

/**
 * Stream the daemon's domain-event SSE (`GET /v1/events`) as newline-delimited
 * JSON: one `DomainEvent` object per line, for `while read` / `jq` pipelines.
 * Resumes from `afterSeq` (the server replays matching backlog, then goes live).
 */
export async function streamEvents(
  conn: Connection,
  opts: EventsOptions,
  deps: EventsDeps,
): Promise<void> {
  await forwardEventStream(
    conn,
    opts,
    { fetch: deps.fetch, signal: deps.signal },
    {
      onEvent: (data, id) => {
        deps.emit(data);
        if (id && deps.log) deps.log(`seq=${id}`);
      },
      onGap: (highestSeq) => {
        deps.log?.(
          `gap: missed events before seq ${highestSeq} were truncated; resuming from ${highestSeq}`,
        );
      },
    },
  );
}
