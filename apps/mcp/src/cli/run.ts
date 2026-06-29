import {
  ApiError,
  ConnectionError,
  type Connection,
  type ConnectionOverrides,
} from "../client.js";
import type { Operation } from "../operations/index.js";
import { parseOperationArgs, UsageError } from "./args.js";
import { streamEvents, type EventsOptions } from "./events.js";
import { commandHelp, topLevelHelp } from "./help.js";

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
  /** Write to stdout (the caller decides whether to append newlines). */
  stdout: (text: string) => void;
  stderr: (text: string) => void;
  /** Whether stdout is a TTY (selects pretty vs compact default output). */
  isTty: boolean;
  env: Record<string, string | undefined>;
  readStdin: () => Promise<string>;
  readFile: (path: string) => Promise<string>;
  fetch: typeof fetch;
  version: string;
  /** Aborts the `events` stream (Ctrl-C). */
  signal?: AbortSignal;
}

interface Globals {
  baseUrl?: string;
  token?: string;
  input?: string;
  output?: "pretty" | "compact";
  help: boolean;
  version: boolean;
}

const EVENTS_HELP = `Usage: posthastectl events [filters]

Stream the daemon's domain events as newline-delimited JSON (one DomainEvent per
line) for 'while read' / 'jq' pipelines. Runs until interrupted.

Filters:
  --after-seq <n>    Resume after this seq (server replays backlog, then live)
  --topic <topic>    Only events with this topic (e.g. sync.completed)
  --account <id>     Only events for this account
  --mailbox <id>     Only events for this mailbox`;

/** Pull the known global flags out of argv, leaving the command tokens. */
function extractGlobals(argv: string[]): { globals: Globals; rest: string[] } {
  const globals: Globals = { help: false, version: false };
  const rest: string[] = [];
  const valued = new Map<string, (v: string) => void>([
    ["--base-url", (v) => (globals.baseUrl = v)],
    ["--token", (v) => (globals.token = v)],
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
        if (next === undefined)
          throw new UsageError(`${name} requires a value`);
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

/** Parse the `events` subcommand's filter flags. */
function parseEventsOptions(tokens: string[]): EventsOptions {
  const opts: EventsOptions = {};
  for (let i = 0; i < tokens.length; i++) {
    const token = tokens[i] ?? "";
    const eq = token.indexOf("=");
    const name = eq >= 0 ? token.slice(0, eq) : token;
    const value = (): string => {
      if (eq >= 0) return token.slice(eq + 1);
      const next = tokens[++i];
      if (next === undefined) throw new UsageError(`${name} requires a value`);
      return next;
    };
    if (name === "--after-seq") {
      const n = Number(value());
      if (!Number.isFinite(n))
        throw new UsageError("--after-seq must be a number");
      opts.afterSeq = n;
    } else if (name === "--topic") opts.topic = value();
    else if (name === "--account") opts.account = value();
    else if (name === "--mailbox") opts.mailbox = value();
    else if (name === "--follow") {
      /* default behavior; accepted for ergonomics */
    } else throw new UsageError(`unknown flag ${name} for 'events'`);
  }
  return opts;
}

/**
 * Run one CLI invocation. `argv` is the args after the program name
 * (`process.argv.slice(2)`). Returns the process exit code; never throws.
 */
export async function run(argv: string[], deps: RunDeps): Promise<number> {
  let globals: Globals;
  let rest: string[];
  try {
    ({ globals, rest } = extractGlobals(argv));
  } catch (error) {
    return failUsage(error, deps);
  }

  if (globals.version) {
    deps.stdout(`posthastectl ${deps.version}\n`);
    return ExitCode.Ok;
  }

  // The `events` tap is a streaming command, not a registry operation.
  if (rest[0] === "events") {
    if (globals.help) {
      deps.stdout(`${EVENTS_HELP}\n`);
      return ExitCode.Ok;
    }
    try {
      const opts = parseEventsOptions(rest.slice(1));
      const conn = connect(deps, globals);
      await streamEvents(conn, opts, {
        fetch: conn.fetch ?? deps.fetch,
        emit: (line) => deps.stdout(`${line}\n`),
        log: (line) => deps.stderr(`posthastectl: ${line}\n`),
        signal: deps.signal,
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
      return rest.length === 0 || globals.help ? ExitCode.Ok : ExitCode.Usage;
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
    const input = globals.input
      ? await resolveInput(globals.input, deps)
      : undefined;
    const args = parseOperationArgs(op, remaining, input);
    const conn = connect(deps, globals);
    const result = await op.handler(conn, args);
    deps.stdout(`${render(result, globals, deps)}\n`);
    return ExitCode.Ok;
  } catch (error) {
    return failRuntime(error, deps);
  }
}

/**
 * Resolve the connection and thread the injectable `fetch` onto it, so every
 * operation handler (via `apiFetch`) and the events tap use the same
 * (test-stubbable) transport.
 */
function connect(deps: RunDeps, globals: Globals): Connection {
  return {
    ...deps.resolveConnection({
      baseUrl: globals.baseUrl,
      token: globals.token,
    }),
    fetch: deps.fetch,
  };
}

function failUsage(error: unknown, deps: RunDeps): number {
  const message = error instanceof Error ? error.message : String(error);
  deps.stderr(`posthastectl: ${message}\n`);
  return ExitCode.Usage;
}

/** Map a thrown error to a diagnostic + meaningful exit code. */
function failRuntime(error: unknown, deps: RunDeps): number {
  if (error instanceof UsageError) {
    deps.stderr(`posthastectl: ${error.message}\n`);
    return ExitCode.Usage;
  }
  if (error instanceof ConnectionError) {
    deps.stderr(`posthastectl: ${error.message}\n`);
    return ExitCode.Connection;
  }
  if (error instanceof ApiError) {
    deps.stderr(`posthastectl: ${error.message}\n`);
    return ExitCode.Api;
  }
  deps.stderr(
    `posthastectl: ${error instanceof Error ? error.message : String(error)}\n`,
  );
  return ExitCode.Unexpected;
}
