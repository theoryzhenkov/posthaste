import { apiFetch, ApiError, type Connection } from "../client.js";
import { consumeSse, gapFrame, openEventStream } from "./events.js";

/**
 * `watch` defaults to subscribing to `message.updated` (the topic that covers
 * both arrivals and metadata changes); the arrival-vs-update distinction is a
 * client-side gate on the payload's `arrivedMailboxIds` and only applies while
 * subscribed to that default topic. `--topic` overrides the subscription — the
 * client-machine-execution pattern (RFC-L2-scripting ruling 19: "evaluate
 * centrally, execute at the edge") pairs an `emit` rule with
 * `watch --topic rule.fired --rule <name> --exec <script>`, which skips the
 * arrival gate entirely (a `rule.fired` fact has no `arrivedMailboxIds`).
 */
const WATCH_TOPIC = "message.updated";

/** Options for the `watch` runner (docs/eph/RFC-L2-scripting.md §7, the ladder, level 2). */
export interface WatchOptions {
  /** Server-side account filter. */
  account?: string;
  /** Server-side topic filter; defaults to `message.updated`. */
  topic?: string;
  /** Client-side mailbox filter (the message must be in this mailbox). */
  mailbox?: string;
  /** Client-side tag filter: the message must carry this JMAP keyword. */
  keyword?: string;
  /**
   * Client-side rule filter (ruling 19): only dispatch `rule.fired` events
   * whose payload `ruleId` matches. CONVENIENCE, NOT A SECURITY BOUNDARY
   * (ruling 20e) — like `--keyword`, this filters what the local script sees,
   * it does not restrict what the server evaluates or emits. Pair with
   * `--topic rule.fired`.
   */
  rule?: string;
  /** Fire on every `message.updated`, not just genuine arrivals. */
  allUpdates?: boolean;
  /** Shell command run per matching message (detail JSON on stdin + env). */
  exec?: string;
  /** File the last-processed `seq` is persisted to, for resume-on-restart. */
  cursorFile?: string;
}

/** Injectable side-effects so the runner is testable without sockets/processes. */
export interface WatchDeps {
  fetch: typeof fetch;
  /** Emit a matching message's detail as one NDJSON line (when no `--exec`). */
  emit: (line: string) => void;
  /** Diagnostics on stderr. */
  log: (line: string) => void;
  /**
   * Run a shell command with `input` on stdin and `env` merged in; resolves with
   * the exit code (never rejects — spawn failures resolve to a non-zero code).
   */
  runCommand?: (
    command: string,
    input: string,
    env: Record<string, string>,
  ) => Promise<number>;
  readFile: (path: string) => Promise<string>;
  writeFile: (path: string, content: string) => Promise<void>;
  signal?: AbortSignal;
}

/** A loosely-typed domain event (the SSE payload; `payload` is topic-specific). */
interface DomainEventish {
  seq?: number;
  accountId?: string;
  messageId?: string;
  topic?: string;
  payload?: {
    messageId?: string;
    arrivedMailboxIds?: unknown;
    /** The firing rule's id, present on `rule.fired` payloads (ruling 19). */
    ruleId?: string;
  };
}

/** A loosely-typed message detail (only the fields `watch` inspects). */
interface MessageDetailish {
  keywords?: unknown;
  mailboxIds?: unknown;
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((v): v is string => typeof v === "string")
    : [];
}

/**
 * Watch for matching mail and run a script per hit. Pipeline per event:
 * arrival-gate → fetch `MessageDetail` (one call: keywords + body + summary) →
 * optional mailbox/keyword filter → dispatch (`--exec` or emit JSON) → advance
 * the resume cursor.
 *
 * Cursor semantics: the cursor advances after **every** event is handled
 * (matched, filtered out, or fetch-failed), so a poison message never wedges the
 * stream and skipped events are not reprocessed. A non-zero `--exec` exit is
 * logged but does not rewind the cursor — make your action idempotent or handle
 * retries in your script. A crash mid-dispatch replays that one event on restart
 * (at-least-once).
 */
export async function watchEvents(
  conn: Connection,
  opts: WatchOptions,
  deps: WatchDeps,
): Promise<void> {
  const afterSeq = opts.cursorFile
    ? await readCursor(opts.cursorFile, deps)
    : undefined;

  const topic = opts.topic ?? WATCH_TOPIC;
  const body = await openEventStream(
    conn,
    { account: opts.account, topic, afterSeq },
    { fetch: deps.fetch, signal: deps.signal },
  );
  if (!body) return;

  await consumeSse(body, (data, _id, event) =>
    handleEvent(data, event, conn, opts, deps),
  );
}

/**
 * Handle one raw SSE frame; always advances the cursor in `finally`.
 *
 * A **gap frame** (see [`gapFrame`]) is handled up front and out-of-band: the
 * missed range is gone, so we log a warning and — if resuming from a cursor —
 * reset that cursor to the log's `highestSeq` (never re-request the lost range).
 * A gap never reaches the arrival gate or the message-detail fetch.
 */
async function handleEvent(
  data: string,
  eventType: string | undefined,
  conn: Connection,
  opts: WatchOptions,
  deps: WatchDeps,
): Promise<void> {
  const gap = gapFrame(data, eventType);
  if (gap) {
    deps.log(
      `gap: missed events before seq ${gap.highestSeq} were truncated; resuming from ${gap.highestSeq}`,
    );
    if (opts.cursorFile) {
      try {
        await deps.writeFile(opts.cursorFile, `${gap.highestSeq}\n`);
      } catch (error) {
        deps.log(`failed to persist cursor: ${String(error)}`);
      }
    }
    return;
  }

  let event: DomainEventish;
  try {
    event = JSON.parse(data) as DomainEventish;
  } catch {
    deps.log("skipping an unparseable event frame");
    return;
  }

  const seq = typeof event.seq === "number" ? event.seq : undefined;
  try {
    // The arrival-vs-update gate only applies to the default `message.updated`
    // subscription — a `--topic rule.fired` (or any other) watch has no
    // `arrivedMailboxIds` to gate on, so every matching event is a "hit".
    const subscribedTopic = opts.topic ?? WATCH_TOPIC;
    if (subscribedTopic === WATCH_TOPIC) {
      const arrived = asStringArray(event.payload?.arrivedMailboxIds);
      if (!opts.allUpdates && arrived.length === 0) return; // not a genuine arrival
    }

    // `--rule` (ruling 19): dispatch only the named rule's `rule.fired` events.
    // CONVENIENCE, NOT A SECURITY BOUNDARY (ruling 20e) — see WatchOptions.rule.
    if (opts.rule && event.payload?.ruleId !== opts.rule) return;

    const accountId = event.accountId;
    const messageId = event.messageId ?? event.payload?.messageId;
    if (!accountId || !messageId) {
      deps.log(`event ${seq ?? "?"}: missing account/message id; skipping`);
      return;
    }

    let detail: MessageDetailish;
    try {
      detail = await apiFetch<MessageDetailish>(
        conn,
        `/sources/${encodeURIComponent(accountId)}/messages/${encodeURIComponent(messageId)}`,
      );
    } catch (error) {
      const reason = error instanceof ApiError ? error.message : String(error);
      deps.log(
        `event ${seq ?? "?"}: fetch ${messageId} failed (${reason}); skipping`,
      );
      return;
    }

    const keywords = asStringArray(detail.keywords);
    const mailboxIds = asStringArray(detail.mailboxIds);
    if (opts.mailbox && !mailboxIds.includes(opts.mailbox)) return;
    if (opts.keyword && !keywords.includes(opts.keyword)) return;

    await dispatch(
      detail,
      {
        seq,
        accountId,
        messageId,
        topic: event.topic,
        ruleId: event.payload?.ruleId,
        keywords,
        mailboxIds,
      },
      opts,
      deps,
    );
  } finally {
    if (opts.cursorFile && seq !== undefined) {
      try {
        await deps.writeFile(opts.cursorFile, `${seq}\n`);
      } catch (error) {
        deps.log(`failed to persist cursor: ${String(error)}`);
      }
    }
  }
}

interface DispatchMeta {
  seq?: number;
  accountId: string;
  messageId: string;
  topic?: string;
  /** The firing rule's id, when the event was a `rule.fired` fact. */
  ruleId?: string;
  keywords: string[];
  mailboxIds: string[];
}

/** Run `--exec` (detail on stdin + env) for a hit, or emit the detail JSON. */
async function dispatch(
  detail: MessageDetailish,
  meta: DispatchMeta,
  opts: WatchOptions,
  deps: WatchDeps,
): Promise<void> {
  if (!opts.exec) {
    deps.emit(JSON.stringify(detail));
    return;
  }
  if (!deps.runCommand) {
    deps.log("no command runner available; cannot honor --exec");
    return;
  }
  const env: Record<string, string> = {
    PH_ACCOUNT_ID: meta.accountId,
    PH_MESSAGE_ID: meta.messageId,
    PH_SEQ: meta.seq !== undefined ? String(meta.seq) : "",
    PH_TOPIC: meta.topic ?? "",
    PH_RULE: meta.ruleId ?? "",
    PH_KEYWORDS: meta.keywords.join(","),
    PH_MAILBOX_IDS: meta.mailboxIds.join(","),
  };
  const code = await deps.runCommand(opts.exec, JSON.stringify(detail), env);
  if (code !== 0) {
    deps.log(
      `--exec exited ${code} for message ${meta.messageId} (seq ${meta.seq ?? "?"})`,
    );
  }
}

/** Read the persisted resume cursor; missing/invalid → start live (undefined). */
async function readCursor(
  file: string,
  deps: WatchDeps,
): Promise<number | undefined> {
  let raw: string;
  try {
    raw = await deps.readFile(file);
  } catch {
    return undefined;
  }
  const seq = Number.parseInt(raw.trim(), 10);
  return Number.isFinite(seq) ? seq : undefined;
}
