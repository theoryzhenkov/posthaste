// `posthastectl events`: the event feed as newline-delimited JSON, one
// EventMessage per line, for `while read` / `jq` pipelines. Filters run
// client-side — the stream is one broadcast, payloads are prompts, and
// anything needing completeness reconciles through queries.

import type { EventMessage } from "@posthaste/protocol/gen";

import type { Connection } from "../core/connection.js";
import { forEachEvent } from "../core/events.js";

/** Client-side filters for the events tap. */
export interface EventsOptions {
  /** Only messages carrying a domain event with this kind. */
  kind?: string;
  /** Only events for this account. */
  account?: string;
  /** Only events for this mailbox. */
  mailbox?: string;
  /**
   * Liveness mode: emit `{generation}` for every message (heartbeats
   * included) and drop event payloads entirely.
   */
  generationOnly?: boolean;
}

/** Injectable side-effects for the tap. */
export interface EventsDeps {
  fetch: typeof fetch;
  /** Emit one line (the newline is added by the caller of this dep). */
  emit: (line: string) => void;
  log?: (line: string) => void;
  signal?: AbortSignal;
}

/** Does an event message pass the client-side filters? */
export function matchesFilters(
  message: EventMessage,
  opts: EventsOptions,
): boolean {
  const event = message.event;
  if (!event) return false; // heartbeat: no payload to filter on
  if (opts.kind && event.kind !== opts.kind) return false;
  if (opts.account && event.accountId !== opts.account) return false;
  if (opts.mailbox && event.mailboxId !== opts.mailbox) return false;
  return true;
}

/**
 * Stream `/events` as NDJSON until the stream ends or `signal` aborts.
 * With `--generation-only` every message (heartbeats included) yields a
 * `{generation}` line; otherwise only messages carrying a matching domain
 * event are emitted, whole. There is no replay: a fresh listen is current by
 * construction (the generation is level-triggered).
 */
export async function streamEvents(
  conn: Connection,
  opts: EventsOptions,
  deps: EventsDeps,
): Promise<void> {
  await forEachEvent(
    conn,
    (message) => {
      if (opts.generationOnly) {
        deps.emit(JSON.stringify({ generation: message.generation }));
        return;
      }
      if (matchesFilters(message, opts)) {
        deps.emit(JSON.stringify(message));
      }
    },
    { fetch: deps.fetch, signal: deps.signal },
  );
}
