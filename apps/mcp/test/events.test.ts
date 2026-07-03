import { describe, expect, test } from "bun:test";

import type { Connection } from "../src/client.js";
import { type EventsDeps, gapFrame, streamEvents } from "../src/cli/events.js";

interface Capture {
  out: string[];
  err: string[];
}

function harness(sse: string): {
  conn: Connection;
  deps: EventsDeps;
  cap: Capture;
} {
  const cap: Capture = { out: [], err: [] };
  const fetch: typeof globalThis.fetch = async () =>
    new Response(sse, { headers: { "content-type": "text/event-stream" } });
  const deps: EventsDeps = {
    fetch,
    emit: (line) => cap.out.push(line),
    log: (line) => cap.err.push(line),
  };
  const conn: Connection = {
    baseUrl: "http://daemon/v1",
    token: undefined,
    source: "flag",
    fetch,
  };
  return { conn, deps, cap };
}

describe("gapFrame", () => {
  test("detects a gap by SSE event type", () => {
    expect(gapFrame('{"kind":"reset","highestSeq":9}', "gap")).toEqual({
      highestSeq: 9,
    });
  });

  test("detects a gap by kind=reset even without the event type", () => {
    expect(gapFrame('{"kind":"reset","highestSeq":3}')).toEqual({
      highestSeq: 3,
    });
  });

  test("an ordinary DomainEvent frame is not a gap", () => {
    expect(gapFrame('{"seq":5,"topic":"message.updated"}')).toBeUndefined();
  });

  test("a non-JSON gap payload still yields a gap (highestSeq 0)", () => {
    expect(gapFrame("not json", "gap")).toEqual({ highestSeq: 0 });
  });
});

describe("streamEvents — gap frame", () => {
  test("logs a warning and does not emit the gap as a data line", async () => {
    const h = harness(
      'event: gap\nid: 9\ndata: {"kind":"reset","highestSeq":9}\n\n',
    );
    await streamEvents(h.conn, {}, h.deps);
    expect(h.cap.out).toHaveLength(0); // gap is never emitted
    expect(h.cap.err.join("\n")).toContain("resuming from 9");
  });

  test("live events after a gap still stream", async () => {
    const h = harness(
      'event: gap\nid: 9\ndata: {"kind":"reset","highestSeq":9}\n\n' +
        'id: 10\ndata: {"seq":10,"topic":"message.updated"}\n\n',
    );
    await streamEvents(h.conn, {}, h.deps);
    expect(h.cap.out).toEqual(['{"seq":10,"topic":"message.updated"}']);
    expect(h.cap.err.join("\n")).toContain("seq=10");
  });
});
