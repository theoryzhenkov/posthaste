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
import { mintToken, parseTokenMintOptions } from "./token.js";
import { watchEvents, type WatchOptions } from "./watch.js";
import {
  parseApplyOptions,
  parseMoveOptions,
  parseReplyOptions,
  parseSendOptions,
  parseTagOptions,
  runApply,
  runMove,
  runReply,
  runSend,
  runTag,
  type WriteVerbDeps,
} from "./writeVerbs.js";

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
  writeFile: (path: string, content: string) => Promise<void>;
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

const TOKEN_HELP = `Usage: posthastectl token mint --grant <scopes> [--expiry <dur>]

Mint a least-privilege capability token by attenuating the auto-discovered
bootstrap token (attenuation happens server-side, so it can only narrow
authority). The token is printed to stdout — so TOKEN=$(posthastectl token mint
...) captures exactly the credential — and a ready-to-paste line to stderr.

Grants (--grant, comma-separated, repeatable):
  tap:read   Subscribe to the event tap (/v1/events)
  read       Bootstrap reads (mail list, message detail, ...)
  apply      Write-back via apply (set-keywords / mailbox moves / destroy)
  <verb>     A raw action verb: read, send, tag, move, delete, manage

Narrowing (optional):
  --account <id>   Restrict to one account (source)
  --mailbox <id>   Restrict to one mailbox
  --message <id>   Restrict to one message
  --expiry <dur>   Lifetime: 3600, 90m, 1h, 7d (recommended for scripts/agents)

Example:
  TOKEN=$(posthastectl token mint --grant tap:read,apply,read --expiry 1h)`;

const TAG_HELP = `Usage: posthastectl tag [--message <id>] [--account <id>] [--add <kw>]... [--remove <kw>]... [--idempotency-key <key>]

Add and/or remove JMAP keywords on a message (POST .../set-keywords). At
least one --add or --remove is required. The typed op, auth, and a
deterministic Idempotency-Key are all handled here — a handler never touches
REST directly.

  --message <id>            Message id (falls back to $PH_MESSAGE_ID)
  --account <id>             Source/account id (falls back to $PH_ACCOUNT / $PH_ACCOUNT_ID)
  --add <kw>                  Repeatable: keyword to add
  --remove <kw>                Repeatable: keyword to remove
  --idempotency-key <key>       Override the auto-derived key (see docs/scripting-quickstart.md)`;

const MOVE_HELP = `Usage: posthastectl move [--message <id>] [--account <id>] --to-mailbox <role|id> [--idempotency-key <key>]

Move a message to a single mailbox (POST .../replace-mailboxes), replacing
its current mailbox memberships. --to-mailbox accepts either a mailbox role
(e.g. 'archive') or a raw mailbox id — roles are resolved via the account's
mailbox list.

  --message <id>            Message id (falls back to $PH_MESSAGE_ID)
  --account <id>             Source/account id (falls back to $PH_ACCOUNT / $PH_ACCOUNT_ID)
  --to-mailbox <role|id>       Destination mailbox
  --idempotency-key <key>       Override the auto-derived key`;

const REPLY_HELP = `Usage: posthastectl reply [--message <id>] [--account <id>] --body <text|-|@file> [--idempotency-key <key>]

Reply in-thread to a message: fetches the gateway's reply-context (recipient,
subject, In-Reply-To/References) and sends --body through it.

  --message <id>            Message id (falls back to $PH_MESSAGE_ID)
  --account <id>             Source/account id (falls back to $PH_ACCOUNT / $PH_ACCOUNT_ID)
  --body <text|-|@file>        The reply body; '-' reads stdin, '@file' reads a file
  --idempotency-key <key>       Override the auto-derived key`;

const SEND_HELP = `Usage: posthastectl send --to <addr>... --subject <s> --body <text|-|@file> [--account <id>] [--cc <addr>]... [--bcc <addr>]... [--from <addr>] [--idempotency-key <key>]

Send a new message (not in reply to anything — see 'reply' for in-thread).

  --account <id>             Source/account id (falls back to $PH_ACCOUNT / $PH_ACCOUNT_ID)
  --to <addr>                 Repeatable: To recipient (required, at least one)
  --cc <addr>                  Repeatable: Cc recipient
  --bcc <addr>                  Repeatable: Bcc recipient
  --from <addr>                  Sender address (defaults to the account's identity)
  --subject <s>                    Subject (required)
  --body <text|-|@file>              Message body; '-' reads stdin, '@file' reads a file
  --idempotency-key <key>              Override the auto-derived key

NOTE: the send route does not yet honor Idempotency-Key server-side (only the
five message-command routes do) — see docs/scripting-quickstart.md.`;

const APPLY_HELP = `Usage: posthastectl apply --kind <kind> [--message <id>] [--account <id>] [--body <json|-|@file>] [--idempotency-key <key>]

Escape hatch: call any of the five typed message-command routes by name, with
a raw JSON body — for verbs without dedicated sugar (destroy,
add-to-mailbox, remove-from-mailbox) or when you want the exact wire shape.

  --kind <kind>              One of: set-keywords, add-to-mailbox, remove-from-mailbox, replace-mailboxes, destroy
  --message <id>              Message id (falls back to $PH_MESSAGE_ID)
  --account <id>                Source/account id (falls back to $PH_ACCOUNT / $PH_ACCOUNT_ID)
  --body <json|-|@file>            The command body (not used by --kind destroy)
  --idempotency-key <key>            Override the auto-derived key`;

/** One write verb: its help text and a parse-then-run entry point. */
interface WriteVerb {
  help: string;
  run: (
    tokens: string[],
    conn: Connection,
    deps: WriteVerbDeps,
  ) => Promise<unknown>;
}

const WRITE_VERBS: Record<string, WriteVerb> = {
  tag: {
    help: TAG_HELP,
    run: (tokens, conn, deps) => runTag(conn, parseTagOptions(tokens), deps),
  },
  move: {
    help: MOVE_HELP,
    run: (tokens, conn, deps) => runMove(conn, parseMoveOptions(tokens), deps),
  },
  reply: {
    help: REPLY_HELP,
    run: (tokens, conn, deps) =>
      runReply(conn, parseReplyOptions(tokens), deps),
  },
  send: {
    help: SEND_HELP,
    run: (tokens, conn, deps) => runSend(conn, parseSendOptions(tokens), deps),
  },
  apply: {
    help: APPLY_HELP,
    run: (tokens, conn, deps) =>
      runApply(conn, parseApplyOptions(tokens), deps),
  },
};

const WATCH_HELP = `Usage: posthastectl watch [filters] [--exec <command>]

Watch for new mail and run a command (or emit JSON) per matching message. For
each genuine arrival it fetches the full message, applies the filters, then runs
--exec with the MessageDetail JSON on stdin and PH_* env vars set
(PH_ACCOUNT_ID, PH_MESSAGE_ID, PH_SEQ, PH_TOPIC, PH_KEYWORDS, PH_MAILBOX_IDS).
Without --exec it prints the matching MessageDetail as one JSON line.

Filters:
  --account <id>     Only this account (server-side)
  --mailbox <id>     Only messages in this mailbox
  --keyword <tag>    Only messages carrying this tag (JMAP keyword)
  --all-updates      Fire on any message change, not just genuine arrivals

Dispatch & resume:
  --exec <command>   Shell command to run per match (detail JSON on stdin)
  --cursor <file>    Persist the last-processed seq here; resume on restart

The --exec command runs on attacker-influenced input (email). The --keyword gate
is convenience, not an auth boundary — treat the payload as untrusted.`;

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

/** Parse the `watch` subcommand's flags. */
function parseWatchOptions(tokens: string[]): WatchOptions {
  const opts: WatchOptions = {};
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
    if (name === "--account") opts.account = value();
    else if (name === "--mailbox") opts.mailbox = value();
    else if (name === "--keyword") opts.keyword = value();
    else if (name === "--exec") opts.exec = value();
    else if (name === "--cursor") opts.cursorFile = value();
    else if (name === "--all-updates") opts.allUpdates = true;
    else throw new UsageError(`unknown flag ${name} for 'watch'`);
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

  // `token mint` — the token-mint UX rider. Bespoke (its `--grant`/`--expiry`
  // surface and human-duration parsing don't match a registry operation's
  // schema-derived flags).
  if (rest[0] === "token") {
    if (globals.help) {
      deps.stdout(`${TOKEN_HELP}\n`);
      return ExitCode.Ok;
    }
    if (rest[1] !== "mint") {
      deps.stderr(`posthastectl: usage: ${TOKEN_HELP}\n`);
      return ExitCode.Usage;
    }
    try {
      const opts = parseTokenMintOptions(rest.slice(2));
      const conn = connect(deps, globals);
      const minted = await mintToken(conn, opts);
      // The bare token on stdout so command-substitution captures only the
      // credential; the paste hint + scope summary go to stderr.
      deps.stdout(`${minted.token}\n`);
      const expiry = minted.expiresAt ? ` (expires ${minted.expiresAt})` : "";
      deps.stderr(
        `posthastectl: minted token for [${opts.actions.join(", ")}]${expiry}\n`,
      );
      deps.stderr(`  export POSTHASTE_TOKEN=${minted.token}\n`);
      deps.stderr(
        `  # or per-invocation: posthastectl --token <token> <command>\n`,
      );
      return ExitCode.Ok;
    } catch (error) {
      return failRuntime(error, deps);
    }
  }

  // The `watch` runner is a streaming command, not a registry operation.
  if (rest[0] === "watch") {
    if (globals.help) {
      deps.stdout(`${WATCH_HELP}\n`);
      return ExitCode.Ok;
    }
    try {
      const opts = parseWatchOptions(rest.slice(1));
      const conn = connect(deps, globals);
      await watchEvents(conn, opts, {
        fetch: conn.fetch ?? deps.fetch,
        emit: (line) => deps.stdout(`${line}\n`),
        log: (line) => deps.stderr(`posthastectl: ${line}\n`),
        runCommand: deps.runCommand,
        readFile: deps.readFile,
        writeFile: deps.writeFile,
        signal: deps.signal,
      });
      return ExitCode.Ok;
    } catch (error) {
      return failRuntime(error, deps);
    }
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

  // The write verbs (RFC-L2-scripting ruling 21: "posthastectl IS the SDK")
  // are bespoke, like `token`/`watch`/`events` above: they need `deps.env` for
  // the PH_*-derived defaults and auto idempotency key, which the registry
  // `Operation.handler(conn, args)` shape does not thread through.
  const writeVerb = WRITE_VERBS[rest[0] ?? ""];
  if (writeVerb) {
    if (globals.help) {
      deps.stdout(`${writeVerb.help}\n`);
      return ExitCode.Ok;
    }
    try {
      const conn = connect(deps, globals);
      const result = await writeVerb.run(rest.slice(1), conn, {
        env: deps.env,
        readStdin: deps.readStdin,
        readFile: deps.readFile,
        warn: (line) => deps.stderr(`posthastectl: ${line}\n`),
      });
      deps.stdout(`${render(result, globals, deps)}\n`);
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
