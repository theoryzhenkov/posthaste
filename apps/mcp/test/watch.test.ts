import { describe, expect, test } from "bun:test";

import type { Connection } from "../src/client.js";
import {
  watchEvents,
  type WatchDeps,
  type WatchOptions,
} from "../src/cli/watch.js";

/** An SSE body with one genuine arrival (m5) and one non-arrival update (m6). */
const SSE =
  'id: 5\ndata: {"seq":5,"accountId":"acct","messageId":"m5","topic":"message.updated","payload":{"messageId":"m5","arrivedMailboxIds":["inbox"]}}\n\n' +
  'id: 6\ndata: {"seq":6,"accountId":"acct","messageId":"m6","topic":"message.updated","payload":{"messageId":"m6","arrivedMailboxIds":[]}}\n\n';

interface Capture {
  out: string[];
  err: string[];
  details: string[];
  commands: { command: string; input: string; env: Record<string, string> }[];
  cursorWrites: string[];
  eventsUrl: string;
}

interface Harness {
  conn: Connection;
  deps: WatchDeps;
  cap: Capture;
}

function harness(
  opts: {
    sse?: string;
    detailFor?: (url: string) => Response;
    cursorContent?: string;
    exitCode?: number;
  } = {},
): Harness {
  const cap: Capture = {
    out: [],
    err: [],
    details: [],
    commands: [],
    cursorWrites: [],
    eventsUrl: "",
  };
  const fetch: typeof globalThis.fetch = async (input) => {
    const url = String(input);
    if (url.includes("/events")) {
      cap.eventsUrl = url;
      return new Response(opts.sse ?? SSE, {
        headers: { "content-type": "text/event-stream" },
      });
    }
    cap.details.push(url);
    return opts.detailFor
      ? opts.detailFor(url)
      : Response.json({
          keywords: ["nebula-command"],
          mailboxIds: ["inbox"],
          bodyText: "hi",
        });
  };
  const deps: WatchDeps = {
    fetch,
    emit: (line) => cap.out.push(line),
    log: (line) => cap.err.push(line),
    runCommand: async (command, input, env) => {
      cap.commands.push({ command, input, env });
      return opts.exitCode ?? 0;
    },
    readFile: async () => {
      if (opts.cursorContent === undefined) throw new Error("ENOENT");
      return opts.cursorContent;
    },
    writeFile: async (_path, content) => {
      cap.cursorWrites.push(content);
    },
  };
  const conn: Connection = {
    baseUrl: "http://daemon/v1",
    token: undefined,
    source: "flag",
    fetch,
  };
  return { conn, deps, cap };
}

async function watch(h: Harness, opts: WatchOptions): Promise<void> {
  await watchEvents(h.conn, opts, h.deps);
}

describe("watch — arrival gating", () => {
  test("only genuine arrivals are processed by default", async () => {
    const h = harness();
    await watch(h, {});
    expect(h.cap.details).toEqual([
      "http://daemon/v1/sources/acct/messages/m5",
    ]);
    expect(h.cap.out).toHaveLength(1);
  });

  test("--all-updates also processes non-arrival changes", async () => {
    const h = harness();
    await watch(h, { allUpdates: true });
    expect(h.cap.details).toHaveLength(2);
    expect(h.cap.out).toHaveLength(2);
  });
});

describe("watch — filters", () => {
  test("--keyword matches a tag on the fetched message", async () => {
    const h = harness();
    await watch(h, { keyword: "nebula-command" });
    expect(h.cap.out).toHaveLength(1);
  });

  test("--keyword skips messages without the tag (still fetched)", async () => {
    const h = harness({
      detailFor: () =>
        Response.json({ keywords: ["other"], mailboxIds: ["inbox"] }),
    });
    await watch(h, { keyword: "nebula-command" });
    expect(h.cap.details).toHaveLength(1); // fetched to inspect
    expect(h.cap.out).toHaveLength(0); // but not dispatched
    expect(h.cap.commands).toHaveLength(0);
  });

  test("--mailbox skips messages not in that mailbox", async () => {
    const h = harness({
      detailFor: () => Response.json({ keywords: [], mailboxIds: ["archive"] }),
    });
    await watch(h, { mailbox: "inbox" });
    expect(h.cap.out).toHaveLength(0);
  });
});

describe("watch — dispatch", () => {
  test("--exec runs the command with detail on stdin and PH_* env", async () => {
    const h = harness();
    await watch(h, { exec: "./to-agent.sh" });
    expect(h.cap.commands).toHaveLength(1);
    const call = h.cap.commands[0]!;
    expect(call.command).toBe("./to-agent.sh");
    expect(JSON.parse(call.input)).toEqual({
      keywords: ["nebula-command"],
      mailboxIds: ["inbox"],
      bodyText: "hi",
    });
    expect(call.env.PH_ACCOUNT_ID).toBe("acct");
    expect(call.env.PH_MESSAGE_ID).toBe("m5");
    expect(call.env.PH_SEQ).toBe("5");
    expect(call.env.PH_KEYWORDS).toBe("nebula-command");
    expect(h.cap.out).toHaveLength(0); // exec mode does not also print
  });

  test("a non-zero --exec exit is logged but does not stop the stream", async () => {
    const h = harness({ exitCode: 3 });
    await watch(h, { exec: "./fails.sh", allUpdates: true });
    expect(h.cap.commands).toHaveLength(2); // both events still dispatched
    expect(h.cap.err.join("\n")).toContain("exited 3");
  });

  test("without --exec, the matching detail is emitted as JSON", async () => {
    const h = harness();
    await watch(h, {});
    expect(JSON.parse(h.cap.out[0]!)).toEqual({
      keywords: ["nebula-command"],
      mailboxIds: ["inbox"],
      bodyText: "hi",
    });
  });
});

describe("watch — cursor (resume)", () => {
  test("reads the cursor into afterSeq and advances it per event", async () => {
    const h = harness({ cursorContent: "4\n" });
    await watch(h, { cursorFile: "/tmp/cur", allUpdates: true });
    expect(h.cap.eventsUrl).toContain("afterSeq=4");
    // Cursor advances for every event seen (m5 then m6).
    expect(h.cap.cursorWrites).toEqual(["5\n", "6\n"]);
  });

  test("a missing cursor file starts live (no afterSeq)", async () => {
    const h = harness();
    await watch(h, { cursorFile: "/tmp/missing" });
    expect(h.cap.eventsUrl).not.toContain("afterSeq");
  });
});

describe("watch — gap frame", () => {
  const GAP_SSE =
    'event: gap\nid: 9\ndata: {"kind":"reset","highestSeq":9}\n\n';

  test("resets the cursor to highestSeq, warns, and fetches no detail", async () => {
    const h = harness({ sse: GAP_SSE, cursorContent: "4\n" });
    await watch(h, { cursorFile: "/tmp/cur" });
    expect(h.cap.details).toHaveLength(0); // gap never reaches detail fetch
    expect(h.cap.out).toHaveLength(0);
    expect(h.cap.cursorWrites).toEqual(["9\n"]); // reset to highestSeq
    expect(h.cap.err.join("\n")).toContain("resuming from 9");
  });

  test("a gap without a cursor file only warns (nothing to persist)", async () => {
    const h = harness({ sse: GAP_SSE });
    await watch(h, {});
    expect(h.cap.details).toHaveLength(0);
    expect(h.cap.cursorWrites).toHaveLength(0);
    expect(h.cap.err.join("\n")).toContain("resuming from 9");
  });

  test("live events after a gap still flow and advance the cursor", async () => {
    const h = harness({
      sse:
        GAP_SSE +
        'id: 10\ndata: {"seq":10,"accountId":"acct","messageId":"m10","topic":"message.updated","payload":{"messageId":"m10","arrivedMailboxIds":["inbox"]}}\n\n',
      cursorContent: "4\n",
    });
    await watch(h, { cursorFile: "/tmp/cur" });
    expect(h.cap.details).toEqual([
      "http://daemon/v1/sources/acct/messages/m10",
    ]);
    expect(h.cap.cursorWrites).toEqual(["9\n", "10\n"]);
  });
});

describe("watch — resilience", () => {
  test("a failed detail fetch is logged and skipped, cursor still advances", async () => {
    const h = harness({
      detailFor: () =>
        Response.json({ code: "not_found", message: "gone" }, { status: 404 }),
    });
    await watch(h, { cursorFile: "/tmp/cur" });
    expect(h.cap.out).toHaveLength(0);
    expect(h.cap.err.join("\n")).toContain("fetch m5 failed");
    expect(h.cap.cursorWrites).toContain("5\n"); // advanced past the bad one
  });
});
