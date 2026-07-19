// Shared test doubles: a recording fake fetch and an SSE byte-stream
// builder, so core/CLI/MCP layers are exercised without sockets.

import type { Connection } from "../src/core/connection.js";

/** One recorded request. */
export interface RecordedRequest {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: unknown;
}

/** A queued response: status + JSON body (or raw text). */
export type QueuedResponse =
  | { status: number; json: unknown }
  | { status: number; text: string };

/** A fake fetch that records requests and replays queued responses. */
export function fakeFetch(responses: QueuedResponse[]): {
  fetch: typeof fetch;
  requests: RecordedRequest[];
} {
  const requests: RecordedRequest[] = [];
  const queue = [...responses];
  const impl = (async (input: string | URL | Request, init?: RequestInit) => {
    const headers: Record<string, string> = {};
    for (const [k, v] of Object.entries((init?.headers ?? {}) as Record<string, string>)) {
      headers[k.toLowerCase()] = v;
    }
    requests.push({
      url: String(input),
      method: init?.method ?? "GET",
      headers,
      body: typeof init?.body === "string" ? JSON.parse(init.body) : undefined,
    });
    const next = queue.shift() ?? { status: 200, json: null };
    const body = "json" in next ? JSON.stringify(next.json) : next.text;
    return new Response(body, {
      status: next.status,
      headers: { "content-type": "application/json" },
    });
  }) as typeof fetch;
  return { fetch: impl, requests };
}

/** A connection wired to a fake fetch. */
export function fakeConnection(fetchImpl: typeof fetch, token = "test-token"): Connection {
  return { baseUrl: "http://127.0.0.1:9", token, source: "env", fetch: fetchImpl };
}

/** Build an SSE byte stream from raw frame strings (each already framed). */
export function sseStream(chunks: string[]): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  return new ReadableStream<Uint8Array>({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
      controller.close();
    },
  });
}

/** A fetch that answers GET /events with an SSE stream, others with JSON. */
export function fakeEventFetch(
  frames: string[],
  jsonResponses: QueuedResponse[] = [],
): { fetch: typeof fetch; requests: RecordedRequest[] } {
  const requests: RecordedRequest[] = [];
  const queue = [...jsonResponses];
  const impl = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = String(input);
    requests.push({
      url,
      method: init?.method ?? "GET",
      headers: {},
      body: typeof init?.body === "string" ? JSON.parse(init.body) : undefined,
    });
    if (url.endsWith("/events")) {
      return new Response(sseStream(frames), {
        status: 200,
        headers: { "content-type": "text/event-stream" },
      });
    }
    const next = queue.shift() ?? { status: 200, json: null };
    const body = "json" in next ? JSON.stringify(next.json) : next.text;
    return new Response(body, { status: next.status });
  }) as typeof fetch;
  return { fetch: impl, requests };
}

/** One SSE frame carrying `data` as its payload. */
export function frame(data: unknown): string {
  return `data: ${JSON.stringify(data)}\n\n`;
}
