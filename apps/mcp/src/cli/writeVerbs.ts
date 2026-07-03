import { randomUUID } from "node:crypto";

import { apiFetch, type Connection } from "../client.js";
import type { components } from "../schema.gen.js";
import { UsageError } from "./args.js";

type Schemas = components["schemas"];

/**
 * `posthastectl {tag,move,reply,send,apply}` — the write half of "posthastectl
 * IS the SDK" (RFC-L2-scripting ruling 21). Each verb constructs the typed
 * command body, resolves auth + idempotency, and hits the right `/v1` route —
 * so a handler shells out to one command and never touches REST, MailOperation,
 * or idempotency math. `run.ts` parses argv into the `*Options` shapes below and
 * calls the matching `run*` function; both are exported separately so tests can
 * exercise flag parsing and HTTP behavior independently.
 *
 * @spec docs/eph/RFC-L2-scripting#north-star-owner-2026-07-03
 */

/** Side-effects a write verb needs beyond the HTTP call itself. */
export interface WriteVerbDeps {
  /** The process environment — source of the `PH_*` convenience fallbacks. */
  env: Record<string, string | undefined>;
  readStdin: () => Promise<string>;
  readFile: (path: string) => Promise<string>;
  /** Diagnostics (e.g. the ad-hoc-idempotency-key warning). */
  warn: (line: string) => void;
}

function envOrUndefined(value: string | undefined): string | undefined {
  return value && value.length > 0 ? value : undefined;
}

/**
 * Resolve `--account`, falling back to the environment so a handler invoked
 * from either exec context never has to pass it explicitly: `PH_ACCOUNT` (the
 * Level-1 rule `exec` action, RFC-L2-scripting ruling 21) then `PH_ACCOUNT_ID`
 * (the Level-2 `watch --exec` runner, `cli/watch.ts`) — both name "the account
 * of the message that triggered this invocation", just under different names
 * from two independently-shipped features.
 */
export function resolveAccount(
  flag: string | undefined,
  env: Record<string, string | undefined>,
): string | undefined {
  return (
    envOrUndefined(flag) ??
    envOrUndefined(env.PH_ACCOUNT) ??
    envOrUndefined(env.PH_ACCOUNT_ID)
  );
}

/** Resolve `--message`, falling back to `PH_MESSAGE_ID` (both exec contexts). */
export function resolveMessage(
  flag: string | undefined,
  env: Record<string, string | undefined>,
): string | undefined {
  return envOrUndefined(flag) ?? envOrUndefined(env.PH_MESSAGE_ID);
}

/**
 * Derive the `Idempotency-Key` for a write verb invocation (RFC-L2-scripting
 * D53 / ruling 21): the handler never computes this by hand.
 *
 * Precedence:
 * 1. `--idempotency-key` (explicit override, always wins).
 * 2. `PH_IDEMPOTENCY_KEY` — already `f(rule, eventSeq)` when this runs as a
 *    Level-1 rule `exec` action (`crates/.../rules/actions.rs`); reused as-is.
 * 3. `PH_EVENT_SEQ` then `PH_SEQ` — the Level-1/Level-2 event-seq env names;
 *    `seq:<n>` is deterministic per triggering event.
 * 4. Otherwise there is no event context (an ad-hoc interactive invocation): a
 *    fresh key is generated and a warning explains that it buys no redelivery
 *    safety — deliberately loud rather than silently non-idempotent.
 *
 * The resolved base is always suffixed with `:<verb>` — a bash handler that
 * calls two verbs for the same event (e.g. `tag` then `move`) must not reuse
 * one literal key across two different operations (the server treats that as
 * a 409 Conflict, RFC-L2-scripting D53), so each verb call gets its own key
 * while both remain a deterministic function of the same triggering event.
 */
export function deriveIdempotencyKey(
  explicit: string | undefined,
  verb: string,
  env: Record<string, string | undefined>,
  warn: (line: string) => void,
): string {
  const override = envOrUndefined(explicit);
  if (override) return override;

  const base =
    envOrUndefined(env.PH_IDEMPOTENCY_KEY) ??
    (envOrUndefined(env.PH_EVENT_SEQ)
      ? `seq:${env.PH_EVENT_SEQ}`
      : undefined) ??
    (envOrUndefined(env.PH_SEQ) ? `seq:${env.PH_SEQ}` : undefined);
  if (base) return `${base}:${verb}`;

  const generated = `cli:${verb}:${randomUUID()}`;
  warn(
    `no PH_EVENT_SEQ/PH_SEQ/PH_IDEMPOTENCY_KEY in the environment; generated an ` +
      `ad-hoc idempotency key (${generated}) that is NOT stable across retries of ` +
      `this invocation. Pass --idempotency-key yourself, or run this from a rule ` +
      `exec / 'watch --exec' handler where the event context is set.`,
  );
  return generated;
}

/** Resolve a body arg (`text` / `-` for stdin / `@file`) — same convention as `--input`. */
export async function resolveBodyArg(
  spec: string,
  deps: Pick<WriteVerbDeps, "readStdin" | "readFile">,
): Promise<string> {
  if (spec === "-") return deps.readStdin();
  if (spec.startsWith("@")) return deps.readFile(spec.slice(1));
  return spec;
}

/** Look up a mailbox by role (`"archive"`) or raw id, for `move --to-mailbox`. */
export async function resolveMailboxId(
  conn: Connection,
  account: string,
  roleOrId: string,
): Promise<string> {
  const mailboxes = await apiFetch<Schemas["MailboxSummary"][]>(
    conn,
    `/sources/${encodeURIComponent(account)}/mailboxes`,
  );
  const byRole = mailboxes.find((m) => m.role === roleOrId);
  if (byRole) return byRole.id;
  const byId = mailboxes.find((m) => m.id === roleOrId);
  if (byId) return byId.id;
  throw new UsageError(
    `no mailbox with role or id '${roleOrId}' for account '${account}' ` +
      `(checked ${mailboxes.length} mailboxes)`,
  );
}

// ---------------------------------------------------------------------------
// tag
// ---------------------------------------------------------------------------

export interface TagOptions {
  account?: string;
  message?: string;
  add: string[];
  remove: string[];
  idempotencyKey?: string;
}

export function parseTagOptions(tokens: string[]): TagOptions {
  const opts: TagOptions = { add: [], remove: [] };
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
    else if (name === "--message") opts.message = value();
    else if (name === "--add") opts.add.push(value());
    else if (name === "--remove") opts.remove.push(value());
    else if (name === "--idempotency-key") opts.idempotencyKey = value();
    else throw new UsageError(`unknown flag ${name} for 'tag'`);
  }
  if (opts.add.length === 0 && opts.remove.length === 0) {
    throw new UsageError("tag requires at least one --add or --remove");
  }
  return opts;
}

export async function runTag(
  conn: Connection,
  opts: TagOptions,
  deps: WriteVerbDeps,
): Promise<Schemas["CommandAck"]> {
  const account = resolveAccount(opts.account, deps.env);
  const message = resolveMessage(opts.message, deps.env);
  if (!account) {
    throw new UsageError(
      "tag: --account is required (or set PH_ACCOUNT / PH_ACCOUNT_ID in the environment)",
    );
  }
  if (!message) {
    throw new UsageError(
      "tag: --message is required (or set PH_MESSAGE_ID in the environment)",
    );
  }
  const key = deriveIdempotencyKey(
    opts.idempotencyKey,
    "tag",
    deps.env,
    deps.warn,
  );
  const body: Schemas["SetKeywordsCommand"] = {
    add: opts.add,
    remove: opts.remove,
  };
  return apiFetch<Schemas["CommandAck"]>(
    conn,
    `/sources/${encodeURIComponent(account)}/commands/messages/${encodeURIComponent(message)}/set-keywords`,
    { method: "POST", body, headers: { "Idempotency-Key": key } },
  );
}

// ---------------------------------------------------------------------------
// move
// ---------------------------------------------------------------------------

export interface MoveOptions {
  account?: string;
  message?: string;
  toMailbox?: string;
  idempotencyKey?: string;
}

export function parseMoveOptions(tokens: string[]): MoveOptions {
  const opts: MoveOptions = {};
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
    else if (name === "--message") opts.message = value();
    else if (name === "--to-mailbox") opts.toMailbox = value();
    else if (name === "--idempotency-key") opts.idempotencyKey = value();
    else throw new UsageError(`unknown flag ${name} for 'move'`);
  }
  if (!opts.toMailbox) {
    throw new UsageError("move requires --to-mailbox <role|id>");
  }
  return opts;
}

export async function runMove(
  conn: Connection,
  opts: MoveOptions,
  deps: WriteVerbDeps,
): Promise<Schemas["CommandAck"]> {
  const account = resolveAccount(opts.account, deps.env);
  const message = resolveMessage(opts.message, deps.env);
  if (!account) {
    throw new UsageError(
      "move: --account is required (or set PH_ACCOUNT / PH_ACCOUNT_ID in the environment)",
    );
  }
  if (!message) {
    throw new UsageError(
      "move: --message is required (or set PH_MESSAGE_ID in the environment)",
    );
  }
  const mailboxId = await resolveMailboxId(
    conn,
    account,
    opts.toMailbox as string,
  );
  const key = deriveIdempotencyKey(
    opts.idempotencyKey,
    "move",
    deps.env,
    deps.warn,
  );
  const body: Schemas["ReplaceMailboxesCommand"] = { mailboxIds: [mailboxId] };
  return apiFetch<Schemas["CommandAck"]>(
    conn,
    `/sources/${encodeURIComponent(account)}/commands/messages/${encodeURIComponent(message)}/replace-mailboxes`,
    { method: "POST", body, headers: { "Idempotency-Key": key } },
  );
}

// ---------------------------------------------------------------------------
// reply
// ---------------------------------------------------------------------------

export interface ReplyOptions {
  account?: string;
  message?: string;
  bodySpec?: string;
  idempotencyKey?: string;
}

export function parseReplyOptions(tokens: string[]): ReplyOptions {
  const opts: ReplyOptions = {};
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
    else if (name === "--message") opts.message = value();
    else if (name === "--body") opts.bodySpec = value();
    else if (name === "--idempotency-key") opts.idempotencyKey = value();
    else throw new UsageError(`unknown flag ${name} for 'reply'`);
  }
  if (!opts.bodySpec) {
    throw new UsageError("reply requires --body <text|-|@file>");
  }
  return opts;
}

/**
 * Reply in-thread: fetches the gateway-computed `reply-context` (recipient,
 * subject, `In-Reply-To`/`References`) for `--message`, then sends `--body`
 * through it. There is no dedicated REST "reply" route — this *is* the
 * composition a manual caller would otherwise hand-roll (RFC-L2-scripting
 * ruling 21: the handler never touches that plumbing itself).
 */
export async function runReply(
  conn: Connection,
  opts: ReplyOptions,
  deps: WriteVerbDeps,
): Promise<Schemas["OkResponse"]> {
  const account = resolveAccount(opts.account, deps.env);
  const message = resolveMessage(opts.message, deps.env);
  if (!account) {
    throw new UsageError(
      "reply: --account is required (or set PH_ACCOUNT / PH_ACCOUNT_ID in the environment)",
    );
  }
  if (!message) {
    throw new UsageError(
      "reply: --message is required (or set PH_MESSAGE_ID in the environment)",
    );
  }
  const body = await resolveBodyArg(opts.bodySpec as string, deps);
  const context = await apiFetch<Schemas["ReplyContext"]>(
    conn,
    `/sources/${encodeURIComponent(account)}/messages/${encodeURIComponent(message)}/reply-context`,
  );
  const key = deriveIdempotencyKey(
    opts.idempotencyKey,
    "reply",
    deps.env,
    deps.warn,
  );
  const request: Schemas["SendMessageRequest"] = {
    to: context.to,
    cc: [],
    bcc: [],
    subject: context.replySubject,
    body,
    inReplyTo: context.inReplyTo ?? undefined,
    references: context.references ?? undefined,
    attachments: [],
  };
  return apiFetch<Schemas["OkResponse"]>(
    conn,
    `/sources/${encodeURIComponent(account)}/commands/send`,
    { method: "POST", body: request, headers: { "Idempotency-Key": key } },
  );
}

// ---------------------------------------------------------------------------
// send
// ---------------------------------------------------------------------------

export interface SendOptions {
  account?: string;
  to: string[];
  cc: string[];
  bcc: string[];
  from?: string;
  subject?: string;
  bodySpec?: string;
  idempotencyKey?: string;
}

export function parseSendOptions(tokens: string[]): SendOptions {
  const opts: SendOptions = { to: [], cc: [], bcc: [] };
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
    else if (name === "--to") opts.to.push(value());
    else if (name === "--cc") opts.cc.push(value());
    else if (name === "--bcc") opts.bcc.push(value());
    else if (name === "--from") opts.from = value();
    else if (name === "--subject") opts.subject = value();
    else if (name === "--body") opts.bodySpec = value();
    else if (name === "--idempotency-key") opts.idempotencyKey = value();
    else throw new UsageError(`unknown flag ${name} for 'send'`);
  }
  if (opts.to.length === 0)
    throw new UsageError("send requires at least one --to");
  if (!opts.subject) throw new UsageError("send requires --subject");
  if (!opts.bodySpec)
    throw new UsageError("send requires --body <text|-|@file>");
  return opts;
}

function toRecipients(addresses: string[]): Schemas["Recipient"][] {
  return addresses.map((email) => ({ email, name: null }));
}

/**
 * Send a new message (not in reply to anything — see `reply` for in-thread).
 *
 * NOTE: `POST /commands/send` does not yet honor `Idempotency-Key` server-side
 * (only the five `commands/messages/{id}/…` routes do, RFC-L2-scripting D53)
 * — the header is still sent for forward-compatibility, but a retried `send`
 * after an ambiguous failure can currently double-send. Not a regression this
 * unit introduces; documented in scripting-quickstart.md.
 */
export async function runSend(
  conn: Connection,
  opts: SendOptions,
  deps: WriteVerbDeps,
): Promise<Schemas["OkResponse"]> {
  const account = resolveAccount(opts.account, deps.env);
  if (!account) {
    throw new UsageError(
      "send: --account is required (or set PH_ACCOUNT / PH_ACCOUNT_ID in the environment)",
    );
  }
  const body = await resolveBodyArg(opts.bodySpec as string, deps);
  const key = deriveIdempotencyKey(
    opts.idempotencyKey,
    "send",
    deps.env,
    deps.warn,
  );
  const request: Schemas["SendMessageRequest"] = {
    from: opts.from ? { email: opts.from, name: null } : undefined,
    to: toRecipients(opts.to),
    cc: toRecipients(opts.cc),
    bcc: toRecipients(opts.bcc),
    subject: opts.subject as string,
    body,
    attachments: [],
  };
  return apiFetch<Schemas["OkResponse"]>(
    conn,
    `/sources/${encodeURIComponent(account)}/commands/send`,
    { method: "POST", body: request, headers: { "Idempotency-Key": key } },
  );
}

// ---------------------------------------------------------------------------
// apply (escape hatch)
// ---------------------------------------------------------------------------

/** The message-command REST routes not (yet) covered by a first-class verb. */
export const APPLY_KINDS = [
  "set-keywords",
  "add-to-mailbox",
  "remove-from-mailbox",
  "replace-mailboxes",
  "destroy",
] as const;
export type ApplyKind = (typeof APPLY_KINDS)[number];

export interface ApplyOptions {
  kind?: ApplyKind;
  account?: string;
  message?: string;
  bodySpec?: string;
  idempotencyKey?: string;
}

export function parseApplyOptions(tokens: string[]): ApplyOptions {
  const opts: ApplyOptions = {};
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
    if (name === "--kind") {
      const v = value();
      if (!(APPLY_KINDS as readonly string[]).includes(v)) {
        throw new UsageError(
          `unknown --kind '${v}' for 'apply' (one of ${APPLY_KINDS.join(", ")})`,
        );
      }
      opts.kind = v as ApplyKind;
    } else if (name === "--account") opts.account = value();
    else if (name === "--message") opts.message = value();
    else if (name === "--body") opts.bodySpec = value();
    else if (name === "--idempotency-key") opts.idempotencyKey = value();
    else throw new UsageError(`unknown flag ${name} for 'apply'`);
  }
  if (!opts.kind) {
    throw new UsageError(
      `apply requires --kind (one of ${APPLY_KINDS.join(", ")})`,
    );
  }
  if (opts.kind !== "destroy" && !opts.bodySpec) {
    throw new UsageError(
      `apply --kind ${opts.kind} requires --body <json|-|@file>`,
    );
  }
  return opts;
}

/**
 * The generic escape hatch: any of the five typed message-command routes by
 * name, with a raw JSON body — for the verbs (`destroy`, `add-to-mailbox`,
 * `remove-from-mailbox`) that don't (yet) have dedicated sugar, or when a
 * handler wants the exact wire shape rather than `tag`/`move`'s convenience
 * mapping. Still typed auth + auto-derived idempotency — never a raw `fetch`
 * in the handler.
 */
export async function runApply(
  conn: Connection,
  opts: ApplyOptions,
  deps: WriteVerbDeps,
): Promise<Schemas["CommandAck"]> {
  const account = resolveAccount(opts.account, deps.env);
  const message = resolveMessage(opts.message, deps.env);
  if (!account) {
    throw new UsageError(
      "apply: --account is required (or set PH_ACCOUNT / PH_ACCOUNT_ID in the environment)",
    );
  }
  if (!message) {
    throw new UsageError(
      "apply: --message is required (or set PH_MESSAGE_ID in the environment)",
    );
  }
  const kind = opts.kind as ApplyKind;
  let body: unknown;
  if (kind !== "destroy") {
    const raw = await resolveBodyArg(opts.bodySpec as string, deps);
    try {
      body = JSON.parse(raw);
    } catch (error) {
      throw new UsageError(
        `--body is not valid JSON: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  }
  const key = deriveIdempotencyKey(
    opts.idempotencyKey,
    `apply:${kind}`,
    deps.env,
    deps.warn,
  );
  return apiFetch<Schemas["CommandAck"]>(
    conn,
    `/sources/${encodeURIComponent(account)}/commands/messages/${encodeURIComponent(message)}/${kind}`,
    { method: "POST", body, headers: { "Idempotency-Key": key } },
  );
}
