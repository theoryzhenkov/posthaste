// `posthastectl watch`: event → query-refetch → dispatch. Each matching
// event triggers a fresh MessageDetail query (state comes from queries, the
// event is only the prompt), then runs `--exec` with the detail JSON on stdin
// and PH_* env vars — payloads and secrets never travel on argv.
//
// Delivery is at-most-once: the broadcast keeps no per-client state and
// missed events are never replayed. A restart (or a dropped connection,
// which `--reconnect` heals by re-listening) can skip events that fired
// while detached — reconcile via queries (e.g. `messages list --is-read
// false`) if your automation needs completeness.

import type {
  EventMessage,
  MessageDetailResult,
} from "@posthaste/protocol/gen";

import type { Connection } from "../core/connection.js";
import { runQuery } from "../core/client.js";
import { forEachEvent } from "../core/events.js";

/** The default kind a watch dispatches on: message metadata/arrival changes. */
const WATCH_KIND = "message.updated";

/** Options for the watch runner. */
export interface WatchOptions {
  /** Only this account's events. */
  account?: string;
  /** Event kind to dispatch on; defaults to `message.updated`. */
  kind?: string;
  /** Only messages currently in this mailbox (checked on the refetched detail). */
  mailbox?: string;
  /** Only messages currently carrying this keyword (checked on the detail). */
  keyword?: string;
  /**
   * Under the default kind, dispatch every update. Without it, a message id
   * dispatches once per watch run (arrival-ish), not on every metadata change.
   */
  allUpdates?: boolean;
  /** Shell command run per match (payload JSON on stdin + PH_* env). */
  exec?: string;
  /** Keep re-listening after the stream drops (with backoff). */
  reconnect?: boolean;
}

/** Injectable side-effects so the runner is testable without sockets. */
export interface WatchDeps {
  fetch: typeof fetch;
  /** Emit a matching payload as one NDJSON line (when no `--exec`). */
  emit: (line: string) => void;
  log: (line: string) => void;
  /** Run a shell command with `input` on stdin and `env` merged in. */
  runCommand?: (
    command: string,
    input: string,
    env: Record<string, string>,
  ) => Promise<number>;
  signal?: AbortSignal;
  /** Sleep between reconnect attempts (injectable for tests). */
  sleep?: (ms: number) => Promise<void>;
}

/** Run `--exec` (payload on stdin, PH_* env) or emit the payload as NDJSON. */
async function dispatch(
  payload: string,
  message: EventMessage,
  opts: WatchOptions,
  deps: WatchDeps,
  extraEnv: Record<string, string>,
): Promise<void> {
  const event = message.event;
  if (opts.exec && deps.runCommand) {
    const env: Record<string, string> = {
      PH_GENERATION: String(message.generation),
      ...(event ? { PH_KIND: event.kind, PH_ACCOUNT_ID: event.accountId } : {}),
      ...extraEnv,
    };
    const code = await deps.runCommand(opts.exec, payload, env);
    if (code !== 0) {
      deps.log(`--exec exited ${code} (generation ${message.generation})`);
    }
    return;
  }
  deps.emit(payload);
}

/** Handle one event message: gate, refetch, filter, dispatch. */
async function handleMessage(
  message: EventMessage,
  conn: Connection,
  opts: WatchOptions,
  deps: WatchDeps,
  seenMessageIds: Set<string>,
): Promise<void> {
  const event = message.event;
  if (!event) return; // heartbeat

  const kind = opts.kind ?? WATCH_KIND;
  if (event.kind !== kind) return;
  if (opts.account && event.accountId !== opts.account) return;

  // Non-message kinds (sync.completed, rule.fired, ...) dispatch the event
  // message itself — there is no message to refetch.
  const messageId = event.messageId;
  if (!messageId) {
    await dispatch(JSON.stringify(message), message, opts, deps, {});
    return;
  }

  // Once-per-run gate under the default kind: without --all-updates, each
  // message dispatches on its first update this run, not on every change.
  if (kind === WATCH_KIND && !opts.allUpdates) {
    if (seenMessageIds.has(messageId)) return;
    seenMessageIds.add(messageId);
  }

  // Refetch: the event is a prompt, the query is the state.
  let detail: MessageDetailResult;
  try {
    const answer = await runQuery<MessageDetailResult>(conn, {
      messageDetail: { accountId: event.accountId, messageId },
    });
    detail = answer.data;
  } catch (error) {
    deps.log(
      `generation ${message.generation}: fetching message failed (${error instanceof Error ? error.message : String(error)}); skipping`,
    );
    return;
  }

  if (opts.mailbox && !detail.summary.mailboxIds.includes(opts.mailbox)) return;
  if (opts.keyword && !detail.summary.keywords.includes(opts.keyword)) return;

  await dispatch(JSON.stringify(detail), message, opts, deps, {
    PH_MESSAGE_ID: messageId,
    PH_KEYWORDS: detail.summary.keywords.join(","),
    PH_MAILBOX_IDS: detail.summary.mailboxIds.join(","),
  });
}

/**
 * Watch the event stream and dispatch per match. With `--reconnect`, a
 * dropped stream is re-listened to with linear backoff — under the
 * level-triggered contract a fresh listen IS the recovery; the missed range
 * is never re-requested (at-most-once).
 */
export async function watchEvents(
  conn: Connection,
  opts: WatchOptions,
  deps: WatchDeps,
): Promise<void> {
  const seen = new Set<string>();
  const sleep =
    deps.sleep ?? ((ms: number) => new Promise((r) => setTimeout(r, ms)));

  for (let attempt = 0; ; attempt++) {
    await forEachEvent(
      conn,
      (message) => handleMessage(message, conn, opts, deps, seen),
      { fetch: deps.fetch, signal: deps.signal },
    );
    if (deps.signal?.aborted || !opts.reconnect) return;
    const delay = Math.min(1000 * (attempt + 1), 15_000);
    deps.log(`event stream ended; re-listening in ${delay}ms`);
    await sleep(delay);
  }
}
