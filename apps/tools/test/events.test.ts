import { describe, expect, test } from "bun:test";

import type { EventMessage } from "@posthaste/protocol/gen";

import {
  awaitGenerationAtLeast,
  consumeSse,
  forEachEvent,
  parseEventMessage,
} from "../src/core/events.js";
import { matchesFilters, streamEvents } from "../src/cli/events.js";
import { watchEvents } from "../src/cli/watch.js";
import { fakeConnection, fakeEventFetch, frame, sseStream } from "./helpers.js";

describe("SSE consumption", () => {
  test("splits frames on blank lines, ignores comments, joins data lines", async () => {
    const seen: string[] = [];
    await consumeSse(
      sseStream([": keep-alive\n\n", "data: {\"a\":1}\n\n", "data: x\ndata: y\n\n"]),
      (data) => {
        seen.push(data);
      },
    );
    expect(seen).toEqual(['{"a":1}', "x\ny"]);
  });

  test("frames split across chunks reassemble", async () => {
    const seen: string[] = [];
    await consumeSse(sseStream(['data: {"gener', 'ation":3}\n', "\n"]), (data) => {
      seen.push(data);
    });
    expect(seen).toEqual(['{"generation":3}']);
  });

  test("parseEventMessage requires a numeric generation", () => {
    expect(parseEventMessage('{"generation":4}')).toEqual({ generation: 4 });
    expect(parseEventMessage("not json")).toBeUndefined();
    expect(parseEventMessage('{"event":{}}')).toBeUndefined();
  });

  test("forEachEvent dispatches typed messages and skips garbage", async () => {
    const { fetch } = fakeEventFetch([
      frame({ generation: 1, runId: "r1" }),
      "data: garbage\n\n",
      frame({ generation: 2, event: { kind: "sync.completed", accountId: "a1" } }),
    ]);
    const generations: number[] = [];
    await forEachEvent(fakeConnection(fetch), (m) => {
      generations.push(m.generation);
    }, { fetch });
    expect(generations).toEqual([1, 2]);
  });
});

describe("awaitGenerationAtLeast", () => {
  test("resolves at the first message at or past the target", async () => {
    const { fetch } = fakeEventFetch([
      frame({ generation: 5 }),
      frame({ generation: 9 }),
      frame({ generation: 11 }),
    ]);
    const seen = await awaitGenerationAtLeast(fakeConnection(fetch), 9, { fetch });
    expect(seen).toBe(9);
  });

  test("rejects when the stream ends short of the target", async () => {
    const { fetch } = fakeEventFetch([frame({ generation: 5 })]);
    await expect(
      awaitGenerationAtLeast(fakeConnection(fetch), 9, { fetch, timeoutMs: 200 }),
    ).rejects.toThrow("ended before generation");
  });
});

describe("events tap", () => {
  const arrival: EventMessage = {
    generation: 10,
    event: { kind: "message.updated", accountId: "a1", messageId: "m1", mailboxId: "mb1" },
  };

  test("client-side filters", () => {
    expect(matchesFilters(arrival, {})).toBe(true);
    expect(matchesFilters(arrival, { kind: "message.updated" })).toBe(true);
    expect(matchesFilters(arrival, { kind: "sync.completed" })).toBe(false);
    expect(matchesFilters(arrival, { account: "a2" })).toBe(false);
    expect(matchesFilters(arrival, { mailbox: "mb1" })).toBe(true);
    expect(matchesFilters({ generation: 3 }, {})).toBe(false); // heartbeat
  });

  test("emits matching events as NDJSON; heartbeats only in --generation-only", async () => {
    const frames = [frame({ generation: 9 }), frame(arrival)];
    {
      const { fetch } = fakeEventFetch(frames);
      const lines: string[] = [];
      await streamEvents(fakeConnection(fetch), {}, { fetch, emit: (l) => lines.push(l) });
      expect(lines).toHaveLength(1);
      expect(JSON.parse(lines[0] ?? "")).toEqual(arrival);
    }
    {
      const { fetch } = fakeEventFetch(frames);
      const lines: string[] = [];
      await streamEvents(
        fakeConnection(fetch),
        { generationOnly: true },
        { fetch, emit: (l) => lines.push(l) },
      );
      expect(lines.map((l) => JSON.parse(l))).toEqual([
        { generation: 9 },
        { generation: 10 },
      ]);
    }
  });
});

describe("watch", () => {
  const detail = {
    summary: {
      id: "m1",
      mailboxIds: ["inbox"],
      keywords: ["urgent"],
    },
    bodyText: "hello",
  };

  test("event triggers a refetch query, filters apply to the detail, --exec gets stdin + PH_* env", async () => {
    const { fetch, requests } = fakeEventFetch(
      [
        frame({ generation: 4 }),
        frame({ generation: 5, event: { kind: "message.updated", accountId: "a1", messageId: "m1" } }),
        frame({ generation: 6, event: { kind: "sync.completed", accountId: "a1" } }),
      ],
      [{ status: 200, json: { generation: 5, data: detail } }],
    );
    const runs: { input: string; env: Record<string, string> }[] = [];
    await watchEvents(
      fakeConnection(fetch),
      { keyword: "urgent", exec: "handler.sh" },
      {
        fetch,
        emit: () => {},
        log: () => {},
        runCommand: async (_cmd, input, env) => {
          runs.push({ input, env });
          return 0;
        },
      },
    );
    // The refetch went through POST /query as a messageDetail family read.
    const queryReq = requests.find((r) => r.url.endsWith("/query"));
    expect(queryReq?.body).toEqual({
      messageDetail: { accountId: "a1", messageId: "m1" },
    });
    expect(runs).toHaveLength(1);
    expect(JSON.parse(runs[0]?.input ?? "")).toEqual(detail);
    expect(runs[0]?.env.PH_GENERATION).toBe("5");
    expect(runs[0]?.env.PH_KIND).toBe("message.updated");
    expect(runs[0]?.env.PH_ACCOUNT_ID).toBe("a1");
    expect(runs[0]?.env.PH_MESSAGE_ID).toBe("m1");
    expect(runs[0]?.env.PH_KEYWORDS).toBe("urgent");
  });

  test("a message dispatches once per run unless --all-updates", async () => {
    const eventFrame = frame({
      generation: 5,
      event: { kind: "message.updated", accountId: "a1", messageId: "m1" },
    });
    const { fetch } = fakeEventFetch(
      [eventFrame, eventFrame],
      [
        { status: 200, json: { generation: 5, data: detail } },
        { status: 200, json: { generation: 5, data: detail } },
      ],
    );
    const lines: string[] = [];
    await watchEvents(fakeConnection(fetch), {}, {
      fetch,
      emit: (l) => lines.push(l),
      log: () => {},
    });
    expect(lines).toHaveLength(1);
  });

  test("a failed refetch is logged and skipped, the stream continues", async () => {
    const { fetch } = fakeEventFetch(
      [
        frame({ generation: 5, event: { kind: "message.updated", accountId: "a1", messageId: "m1" } }),
        frame({ generation: 6, event: { kind: "message.updated", accountId: "a1", messageId: "m2" } }),
      ],
      [
        { status: 404, json: { kind: "unknownId", message: "gone", retryable: false } },
        { status: 200, json: { generation: 6, data: detail } },
      ],
    );
    const lines: string[] = [];
    const logs: string[] = [];
    await watchEvents(fakeConnection(fetch), {}, {
      fetch,
      emit: (l) => lines.push(l),
      log: (l) => logs.push(l),
    });
    expect(lines).toHaveLength(1);
    expect(logs.some((l) => l.includes("skipping"))).toBe(true);
  });

  test("non-message kinds dispatch the event message itself", async () => {
    const sync = { generation: 6, event: { kind: "sync.completed", accountId: "a1" } };
    const { fetch } = fakeEventFetch([frame(sync)]);
    const lines: string[] = [];
    await watchEvents(fakeConnection(fetch), { kind: "sync.completed" }, {
      fetch,
      emit: (l) => lines.push(l),
      log: () => {},
    });
    expect(lines.map((l) => JSON.parse(l))).toEqual([sync]);
  });
});
