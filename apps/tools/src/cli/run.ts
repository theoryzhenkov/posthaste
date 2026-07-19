// The CLI dispatcher: global flags, the streaming commands (events/watch),
// and the registry operations rendered as subcommands. Pure over injected
// side-effects, so every path is testable without processes or sockets.

import type { Connection, ConnectionOverrides } from "../core/connection.js";
import { ConnectionError } from "../core/connection.js";
import { ApiCallError, TransportError } from "../core/errors.js";
import type { Operation } from "../operations/index.js";
import { parseOperationArgs, UsageError } from "./args.js";
import { streamEvents, type EventsOptions } from "./events.js";
import { commandHelp, topLevelHelp } from "./help.js";
import { watchEvents, type WatchOptions } from "./watch.js";

/**
 * Exit codes (the scriptable contract). `0` success; everything else carries
 * meaning so scripts can branch on `$?`.
 */
export const ExitCode = {
  Ok: 0,
  Unexpected: 1,
  Usage: 2,
  Connection: 3,
  Api: 4,
} as const;

/** Side-effects + environment, injected so `run` is pure and testable. */
export interface RunDeps {
  operations: Operation[];
  resolveConnection: (overrides: ConnectionOverrides) => Connection;
  stdout: (text: string) => void;
  stderr: (text: string) => void;
  /** Whether stdout is a TTY (selects pretty vs compact default output). */
  isTty: boolean;
  env: Record<string, string | undefined>;
  readStdin: () => Promise<string>;
  readFile: (path: string) => Promise<string>;
  fetch: typeof fetch;
  /** Run a `watch --exec` command; resolves with the exit code (never rejects). */
  runCommand?: (
    command: string,
    input: string,
    env: Record<string, string>,
  ) => Promise<number>;
  version: string;
  /** Aborts the `events`/`watch` stream (Ctrl-C). */
  signal?: AbortSignal;
}

interface Globals {
  baseUrl?: string;
  input?: string;
  output?: "pretty" | "compact";
  help: boolean;
  version: boolean;
}

const EVENTS_HELP = `Usage: posthastectl events [filters]

Stream the backend's event feed as newline-delimited JSON (one EventMessage
per line) for 'while read' / 'jq' pipelines. Runs until interrupted.

Every message carries the current store generation; most carry a domain
event. There is NO replay of missed events — the generation is
level-triggered, so a fresh listen is current by construction; reconcile
through queries when completeness matters.

Filters (client-side):
  --kind <kind>       Only events of this kind (e.g. message.updated,
                      sync.completed, operation.settled)
  --account <id>      Only events for this account
  --mailbox <id>      Only events for this mailbox
  --generation-only   Liveness mode: emit {generation} for every message
                      (heartbeats included), no event payloads`;

const WATCH_HELP = `Usage: posthastectl watch [filters] [--exec <command>]

Watch the event feed and run a command (or emit JSON) per matching event.
Defaults to --kind message.updated: each event triggers a FRESH message
query (the event is a prompt; state comes from queries), the filters apply
to the refetched detail, then --exec runs with the MessageDetail JSON on
stdin and PH_* env set (PH_GENERATION, PH_KIND, PH_ACCOUNT_ID,
PH_MESSAGE_ID, PH_KEYWORDS, PH_MAILBOX_IDS). Without --exec it prints the
matching detail as one JSON line.

Delivery is at-most-once: a watch that was detached (restart, dropped
stream) does not replay what it missed — reconcile via queries (e.g.
'messages list --is-read false') when completeness matters.

Filters:
  --account <id>     Only this account
  --kind <kind>      Dispatch on this event kind instead of message.updated
  --mailbox <id>     Only messages currently in this mailbox
  --keyword <kw>     Only messages currently carrying this keyword
  --all-updates      Fire on every update, not once per message

Dispatch:
  --exec <command>   Shell command per match (payload on stdin, PH_* env;
                     payloads and secrets never on argv)
  --reconnect        Re-listen with backoff when the stream drops

The --exec command runs on attacker-influenced input (email). Treat the
payload as untrusted data; the filters are convenience, not a security
boundary.`;

/** Pull the known global flags out of argv, leaving the command tokens. */
function extractGlobals(argv: string[]): { globals: Globals; rest: string[] } {
  const globals: Globals = { help: false, version: false };
  const rest: string[] = [];
  const valued = new Map<string, (v: string) => void>([
    ["--base-url", (v) => (globals.baseUrl = v)],
    ["--input", (v) => (globals.input = v)],
    ["-i", (v) => (globals.input = v)],
  ]);

  for (let i = 0; i < argv.length; i++) {
    const token = argv[i] ?? "";
    const eq = token.indexOf("=");
    const name = eq >= 0 ? token.slice(0, eq) : token;
    const setter = valued.get(name);
    if (setter) {
      if (eq >= 0) setter(token.slice(eq + 1));
      else {
        const next = argv[++i];
        if (next === undefined) throw new UsageError(`${name} requires a value`);
        setter(next);
      }
      continue;
    }
    if (token === "--compact" || token === "--json") globals.output = "compact";
    else if (token === "--pretty") globals.output = "pretty";
    else if (token === "--help" || token === "-h") globals.help = true;
    else if (token === "--version" || token === "-V") globals.version = true;
    else rest.push(token);
  }
  return { globals, rest };
}

/** Match the longest operation path that prefixes the leading command tokens. */
function matchOperation(
  operations: Operation[],
  rest: string[],
): { op: Operation; remaining: string[] } | undefined {
  const leading: string[] = [];
  for (const token of rest) {
    if (token.startsWith("-")) break;
    leading.push(token);
  }
  let best: Operation | undefined;
  let bestLen = 0;
  for (const op of operations) {
    const path = op.cli.path;
    if (
      path.length <= leading.length &&
      path.length > bestLen &&
      path.every((seg, i) => seg === leading[i])
    ) {
      best = op;
      bestLen = path.length;
    }
  }
  return best ? { op: best, remaining: rest.slice(bestLen) } : undefined;
}

/** Resolve a `--input` spec (inline JSON / `-` stdin / `@file`) to an object. */
async function resolveInput(
  spec: string,
  deps: RunDeps,
): Promise<Record<string, unknown>> {
  let raw: string;
  if (spec === "-") raw = await deps.readStdin();
  else if (spec.startsWith("@")) raw = await deps.readFile(spec.slice(1));
  else raw = spec;

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch (error) {
    throw new UsageError(
      `--input is not valid JSON: ${error instanceof Error ? error.message : String(error)}`,
    );
  }
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    throw new UsageError("--input must be a JSON object");
  }
  return parsed as Record<string, unknown>;
}

/** Render an operation result as JSON (pretty on a TTY, compact when piped). */
function render(result: unknown, globals: Globals, deps: RunDeps): string {
  const pretty = globals.output ? globals.output === "pretty" : deps.isTty;
  return JSON.stringify(result ?? null, null, pretty ? 2 : undefined);
}

/** Generic single-pass flag parser for the streaming commands. */
function parseFlags(
  tokens: string[],
  command: string,
  valued: Record<string, (v: string) => void>,
  bare: Record<string, () => void>,
): void {
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i] ?? "";
    const eq = token.indexOf("=");
    const name = eq >= 0 ? token.slice(0, eq) : token;
    const setValued = valued[name];
    if (setValued) {
      if (eq >= 0) setValued(token.slice(eq + 1));
      else {
        const next = tokens[++i];
        if (next === undefined)
          throw new UsageError(`${name} requires a value`);
        setValued(next);
      }
      continue;
    }
    const setBare = bare[name];
    if (setBare) {
      setBare();
      continue;
    }
    throw new UsageError(`unknown flag ${name} for '${command}'`);
  }
}

/** Parse the `events` subcommand's filter flags. */
function parseEventsOptions(tokens: string[]): EventsOptions {
  const opts: EventsOptions = {};
  parseFlags(
    tokens,
    "events",
    {
      "--kind": (v) => (opts.kind = v),
      "--account": (v) => (opts.account = v),
      "--mailbox": (v) => (opts.mailbox = v),
    },
    {
      "--generation-only": () => (opts.generationOnly = true),
    },
  );
  return opts;
}

/** Parse the `watch` subcommand's flags. */
function parseWatchOptions(tokens: string[]): WatchOptions {
  const opts: WatchOptions = {};
  parseFlags(
    tokens,
    "watch",
    {
      "--account": (v) => (opts.account = v),
      "--kind": (v) => (opts.kind = v),
      "--mailbox": (v) => (opts.mailbox = v),
      "--keyword": (v) => (opts.keyword = v),
      "--exec": (v) => (opts.exec = v),
    },
    {
      "--all-updates": () => (opts.allUpdates = true),
      "--reconnect": () => (opts.reconnect = true),
    },
  );
  return opts;
}

/**
 * Run one CLI invocation. `argv` is the args after the program name
 * (`process.argv.slice(2)`). Returns the process exit code; never throws.
 */
// Injected at compile time by scripts/build-cli.ts (--define); undefined in
// dev/tests. Release smokes run `<binary> --print-release-channel` on every
// bundled executable to catch channel mix-ups.
declare const POSTHASTE_BUILD_CHANNEL: string | undefined;

export async function run(argv: string[], deps: RunDeps): Promise<number> {
  if (argv[0] === "--print-release-channel") {
    const channel =
      typeof POSTHASTE_BUILD_CHANNEL === "string" ? POSTHASTE_BUILD_CHANNEL : "";
    if (channel === "") return ExitCode.Usage;
    deps.stdout(`${channel}\n`);
    return ExitCode.Ok;
  }

  let globals: Globals;
  let rest: string[];
  try {
    ({ globals, rest } = extractGlobals(argv));
  } catch (error) {
    return failRuntime(error, deps);
  }

  if (globals.version) {
    deps.stdout(`posthastectl ${deps.version}\n`);
    return ExitCode.Ok;
  }

  // The streaming commands are not registry operations.
  if (rest[0] === "events") {
    if (globals.help) {
      deps.stdout(`${EVENTS_HELP}\n`);
      return ExitCode.Ok;
    }
    try {
      const opts = parseEventsOptions(rest.slice(1));
      const conn = connect(deps, globals);
      await streamEvents(conn, opts, {
        fetch: deps.fetch,
        emit: (line) => deps.stdout(`${line}\n`),
        log: (line) => deps.stderr(`posthastectl: ${line}\n`),
        ...(deps.signal ? { signal: deps.signal } : {}),
      });
      return ExitCode.Ok;
    } catch (error) {
      return failRuntime(error, deps);
    }
  }

  if (rest[0] === "watch") {
    if (globals.help) {
      deps.stdout(`${WATCH_HELP}\n`);
      return ExitCode.Ok;
    }
    try {
      const opts = parseWatchOptions(rest.slice(1));
      if (opts.exec && !deps.runCommand) {
        throw new UsageError("no command runner available; cannot honor --exec");
      }
      const conn = connect(deps, globals);
      await watchEvents(conn, opts, {
        fetch: deps.fetch,
        emit: (line) => deps.stdout(`${line}\n`),
        log: (line) => deps.stderr(`posthastectl: ${line}\n`),
        ...(deps.runCommand ? { runCommand: deps.runCommand } : {}),
        ...(deps.signal ? { signal: deps.signal } : {}),
      });
      return ExitCode.Ok;
    } catch (error) {
      return failRuntime(error, deps);
    }
  }

  const match = matchOperation(deps.operations, rest);
  if (!match) {
    if (globals.help || rest.length === 0) {
      deps.stdout(`${topLevelHelp(deps.operations)}\n`);
      return ExitCode.Ok;
    }
    deps.stderr(`posthastectl: unknown command '${rest.join(" ")}'\n\n`);
    deps.stderr(`${topLevelHelp(deps.operations)}\n`);
    return ExitCode.Usage;
  }

  const { op, remaining } = match;
  if (globals.help) {
    deps.stdout(`${commandHelp(op)}\n`);
    return ExitCode.Ok;
  }

  try {
    const input = globals.input ? await resolveInput(globals.input, deps) : undefined;
    const args = parseOperationArgs(op, remaining, input);
    const conn = connect(deps, globals);
    const result = await op.handler(conn, args);
    deps.stdout(`${render(result, globals, deps)}\n`);
    return ExitCode.Ok;
  } catch (error) {
    return failRuntime(error, deps);
  }
}

/** Resolve the connection and thread the injectable `fetch` onto it. */
function connect(deps: RunDeps, globals: Globals): Connection {
  const overrides: ConnectionOverrides = {};
  if (globals.baseUrl) overrides.baseUrl = globals.baseUrl;
  return { ...deps.resolveConnection(overrides), fetch: deps.fetch };
}

/** Map a thrown error to a diagnostic + meaningful exit code. */
function failRuntime(error: unknown, deps: RunDeps): number {
  if (error instanceof UsageError) {
    deps.stderr(`posthastectl: ${error.message}\n`);
    return ExitCode.Usage;
  }
  if (error instanceof ConnectionError || error instanceof TransportError) {
    deps.stderr(`posthastectl: ${error.message}\n`);
    return ExitCode.Connection;
  }
  if (error instanceof ApiCallError) {
    deps.stderr(`posthastectl: ${error.message}\n`);
    if (error.retryable) {
      deps.stderr("posthastectl: (retryable — the same request may succeed shortly)\n");
    }
    return ExitCode.Api;
  }
  deps.stderr(
    `posthastectl: ${error instanceof Error ? error.message : String(error)}\n`,
  );
  return ExitCode.Unexpected;
}
