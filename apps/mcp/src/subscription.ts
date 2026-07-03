import type { Connection } from "./client.js";
import { forwardEventStream, type EventsOptions } from "./cli/events.js";

/**
 * The MCP subscription: the second half of RFC-L2-scripting ruling 22. A
 * persistent agent connects once and, alongside the action tools, is *pushed*
 * the fact stream — the daemon's `/v1/events` tap surfaced as MCP
 * `notifications/message` (the standard, capability-gated server→client push).
 * The event body rides in the notification's structured `data`; the `logger`
 * and a `kind` discriminator let the agent route on it, and a distinct `level`
 * makes a gap frame impossible to mistake for an ordinary event.
 *
 * We reuse the CLI tap's SSE primitives verbatim (`forwardEventStream` →
 * `openEventStream`/`consumeSse`/`gapFrame`); this module only decides how a
 * frame becomes a notification.
 *
 * @spec docs/eph/RFC-L2-scripting.md ruling 22
 */

/** MCP logging levels we emit (a subset of the SDK's syslog-style set). */
export type NotificationLevel = "info" | "warning" | "error";

/**
 * The structured `data` payload carried on each `notifications/message`. A
 * `kind` discriminates an ordinary event from a gap so the agent branches
 * without inspecting the level.
 */
export type PosthasteNotificationData =
  | {
      kind: "event";
      /** The event topic (e.g. `rule.fired`, `rule.delivery.failed`). */
      topic: string | undefined;
      /** The event seq (the resume cursor), when present. */
      seq: number | undefined;
      /** The full `DomainEvent` (parsed), or the raw string if it was not JSON. */
      event: unknown;
    }
  | {
      /**
       * The tap's durability signal: history before `highestSeq` was truncated,
       * so a lagged agent must reconcile (re-read state) rather than assume it
       * saw every event. Surfaced as a distinct notification, never a silent drop.
       */
      kind: "gap";
      highestSeq: number;
    };

/** One agent-bound notification, shaped to map 1:1 onto `sendLoggingMessage`. */
export interface EventNotification {
  level: NotificationLevel;
  /** The MCP `logger` tag — always `"posthaste"`, so an agent can filter. */
  logger: string;
  data: PosthasteNotificationData;
}

/** Injectable side-effects for the subscription. */
export interface SubscriptionDeps {
  fetch: typeof fetch;
  /** Deliver one notification to the connected agent. */
  send: (notification: EventNotification) => Promise<void> | void;
  /** Diagnostics to stderr. */
  log?: (line: string) => void;
  signal?: AbortSignal;
}

/**
 * Topics that are the *point* of the subscription (ruling 22): a rule firing,
 * or a rule's delivery dead-lettering. A failed delivery is pushed at `error`
 * level so an agent (or its host) surfaces it prominently; everything else is
 * `info`.
 */
function levelForTopic(topic: string | undefined): NotificationLevel {
  return topic === "rule.delivery.failed" ? "error" : "info";
}

/**
 * Subscribe to the daemon event tap and forward each frame to `deps.send` as an
 * [`EventNotification`]. Resumes from `opts.afterSeq` when the session supplies
 * a last-seen cursor, else attaches at the live head (snapshot-attach, §5.3).
 * Runs until the stream ends or `deps.signal` aborts.
 */
export async function subscribeEvents(
  conn: Connection,
  opts: EventsOptions,
  deps: SubscriptionDeps,
): Promise<void> {
  await forwardEventStream(
    conn,
    opts,
    { fetch: deps.fetch, signal: deps.signal },
    {
      onEvent: async (data, id) => {
        let event: unknown;
        try {
          event = JSON.parse(data);
        } catch {
          event = data;
        }
        const topic =
          typeof event === "object" && event !== null
            ? (event as { topic?: unknown }).topic
            : undefined;
        const seqRaw =
          typeof event === "object" && event !== null
            ? (event as { seq?: unknown }).seq
            : undefined;
        const seq =
          typeof seqRaw === "number"
            ? seqRaw
            : id !== undefined && id.length > 0
              ? Number(id)
              : undefined;
        const topicStr = typeof topic === "string" ? topic : undefined;
        await deps.send({
          level: levelForTopic(topicStr),
          logger: "posthaste",
          data: {
            kind: "event",
            topic: topicStr,
            seq: seq !== undefined && Number.isFinite(seq) ? seq : undefined,
            event,
          },
        });
      },
      onGap: async (highestSeq) => {
        deps.log?.(
          `gap: history before seq ${highestSeq} was truncated; agent must reconcile`,
        );
        await deps.send({
          level: "warning",
          logger: "posthaste",
          data: { kind: "gap", highestSeq },
        });
      },
    },
  );
}
