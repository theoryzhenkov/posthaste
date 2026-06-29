import { ApiError, type Connection } from "../client.js";

/** Filters for the domain-event stream (mirror of the `EventFilter` query). */
export interface EventsOptions {
  afterSeq?: number;
  topic?: string;
  account?: string;
  mailbox?: string;
}

/** Injectable side-effects, so the tap is testable without real sockets. */
export interface EventsDeps {
  fetch: typeof fetch;
  /** Emit one event as a single NDJSON line (the newline is added by the caller). */
  emit: (line: string) => void;
  /** Diagnostics (e.g. the resolved last seq). */
  log?: (line: string) => void;
  signal?: AbortSignal;
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
 * Stream the daemon's domain-event SSE (`GET /v1/events`) as newline-delimited
 * JSON: one `DomainEvent` object per line, for `while read` / `jq` pipelines.
 * Resumes from `afterSeq` (the server replays matching backlog, then goes live).
 *
 * Resolves only when the stream ends or `signal` aborts; rejects with an
 * [`ApiError`] on a non-2xx open or a transport failure.
 */
export async function streamEvents(
  conn: Connection,
  opts: EventsOptions,
  deps: EventsDeps,
): Promise<void> {
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
    if (deps.signal?.aborted) return;
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

  if (!res.body) return;
  await consumeSse(res.body, deps);
}

/**
 * Parse an SSE byte stream into `data:` payloads. Each complete event (frames
 * separated by a blank line) is emitted as one NDJSON line; the SSE `id:`
 * (the event seq) is reported via `log` for resume diagnostics.
 */
async function consumeSse(
  body: ReadableStream<Uint8Array>,
  deps: EventsDeps,
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
        emitFrame(frame, deps);
      }
    }
  } finally {
    reader.releaseLock();
  }
}

/** Emit a single parsed SSE frame's data payload as one NDJSON line. */
function emitFrame(frame: string, deps: EventsDeps): void {
  const dataLines: string[] = [];
  let id: string | undefined;
  for (const line of frame.split("\n")) {
    if (line.startsWith(":")) continue; // comment / keep-alive
    if (line.startsWith("data:")) dataLines.push(line.slice(5).trimStart());
    else if (line.startsWith("id:")) id = line.slice(3).trim();
  }
  if (dataLines.length === 0) return;
  const data = dataLines.join("\n");
  deps.emit(data);
  if (id && deps.log) deps.log(`seq=${id}`);
}
