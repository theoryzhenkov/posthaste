import { describe, expect, test } from "bun:test";

import type { Connection } from "../src/client.js";
import {
  subscribeEvents,
  type EventNotification,
  type SubscriptionDeps,
} from "../src/subscription.js";

/** A subscription harness whose tap `fetch` replays a fixed SSE body. */
function harness(sse: string): {
  conn: Connection;
  deps: SubscriptionDeps;
  sent: EventNotification[];
  logs: string[];
} {
  const sent: EventNotification[] = [];
  const logs: string[] = [];
  const fetch: typeof globalThis.fetch = async () =>
    new Response(sse, { headers: { "content-type": "text/event-stream" } });
  const conn: Connection = {
    baseUrl: "http://daemon/v1",
    token: "scoped",
    source: "daemon.json",
    fetch,
  };
  const deps: SubscriptionDeps = {
    fetch,
    send: (n) => {
      sent.push(n);
    },
    log: (line) => logs.push(line),
  };
  return { conn, deps, sent, logs };
}

describe("subscribeEvents — forwarding facts as notifications", () => {
  test("a seeded rule.fired event surfaces as an event notification", async () => {
    const event = {
      seq: 91,
      topic: "rule.fired",
      accountId: "acct-a",
      messageId: "msg-1",
      occurredAt: "2026-07-03T00:00:00Z",
      payload: {},
    };
    const h = harness(`id: 91\ndata: ${JSON.stringify(event)}\n\n`);
    await subscribeEvents(h.conn, {}, h.deps);

    expect(h.sent).toHaveLength(1);
    const n = h.sent[0]!;
    expect(n.level).toBe("info");
    expect(n.logger).toBe("posthaste");
    expect(n.data.kind).toBe("event");
    if (n.data.kind !== "event") throw new Error("expected an event");
    expect(n.data.topic).toBe("rule.fired");
    expect(n.data.seq).toBe(91);
    expect(n.data.event).toEqual(event);
  });

  test("rule.delivery.failed is pushed at error level", async () => {
    const event = { seq: 92, topic: "rule.delivery.failed", payload: {} };
    const h = harness(`id: 92\ndata: ${JSON.stringify(event)}\n\n`);
    await subscribeEvents(h.conn, {}, h.deps);

    expect(h.sent).toHaveLength(1);
    expect(h.sent[0]!.level).toBe("error");
  });

  test("a gap frame surfaces distinctly (kind=gap, warning) and is logged", async () => {
    const h = harness(
      'event: gap\nid: 9\ndata: {"kind":"reset","highestSeq":9}\n\n' +
        'id: 10\ndata: {"seq":10,"topic":"message.updated","payload":{}}\n\n',
    );
    await subscribeEvents(h.conn, {}, h.deps);

    expect(h.sent).toHaveLength(2);
    const gap = h.sent[0]!;
    expect(gap.level).toBe("warning");
    expect(gap.data.kind).toBe("gap");
    if (gap.data.kind !== "gap") throw new Error("expected a gap");
    expect(gap.data.highestSeq).toBe(9);
    // The live event after the gap still forwards as an ordinary event.
    expect(h.sent[1]!.data.kind).toBe("event");
    expect(h.logs.join("\n")).toContain("must reconcile");
  });

  test("respects the resume cursor (afterSeq) on the tap request", async () => {
    let requested: string | undefined;
    const fetch: typeof globalThis.fetch = async (input) => {
      requested = String(input);
      return new Response(
        'id: 5\ndata: {"seq":5,"topic":"x","payload":{}}\n\n',
        {
          headers: { "content-type": "text/event-stream" },
        },
      );
    };
    const conn: Connection = {
      baseUrl: "http://daemon/v1",
      token: "scoped",
      source: "daemon.json",
      fetch,
    };
    await subscribeEvents(conn, { afterSeq: 4 }, { fetch, send: () => {} });
    expect(requested).toContain("afterSeq=4");
  });
});
