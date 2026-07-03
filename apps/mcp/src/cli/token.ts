import { apiFetch, type Connection } from "../client.js";
import { UsageError } from "./args.js";

/**
 * `posthastectl token mint` — the one-command token-mint UX (RFC-L2-scripting
 * slice-1 rider). Attenuates the auto-discovered bootstrap token into a
 * least-privilege, optionally-expiring token by calling the server's mint route
 * (`POST /v1/auth/tokens`), which does the macaroon attenuation server-side so
 * the result can only narrow the caller's authority.
 *
 * @spec docs/eph/RFC-L2-scripting#6-d53-the-action-path
 */

/** The server `Action` verbs the mint route accepts (the wire vocabulary). */
const ACTION_VERBS = [
  "read",
  "send",
  "tag",
  "move",
  "delete",
  "manage",
] as const;

/** Resolved options for `token mint` after flag parsing. */
export interface TokenMintOptions {
  /** Deduped server action verbs, resolved from the `--grant` scopes. */
  actions: string[];
  expiresInSeconds?: number;
  account?: string;
  mailbox?: string;
  message?: string;
}

/** The minted-token response (mirrors `CreateAuthTokenResponse`). */
export interface MintedToken {
  token: string;
  expiresAt?: string;
}

/**
 * Map a `--grant` scope to the underlying server `Action` verbs. Accepts the
 * conceptual grants from the RFC (`tap:read` / `read` / `apply`) as sugar over
 * the wire vocabulary, plus the raw verbs for full control. The tap and bootstrap
 * reads are both `read`-gated routes; `apply` covers the direct write-back
 * command verbs (set-keywords = tag, mailbox moves = move, destroy = delete).
 */
function grantToActions(grant: string): string[] {
  switch (grant) {
    case "tap:read":
    case "tap":
      return ["read"];
    case "read":
      return ["read"];
    case "apply":
      return ["tag", "move", "delete"];
    default:
      if ((ACTION_VERBS as readonly string[]).includes(grant)) return [grant];
      throw new UsageError(
        `unknown grant '${grant}' — use tap:read, read, apply, or a raw verb ` +
          `(${ACTION_VERBS.join(", ")})`,
      );
  }
}

const DURATION = /^(\d+)\s*([smhd]?)$/;
const UNIT_SECONDS: Record<string, number> = {
  "": 1,
  s: 1,
  m: 60,
  h: 3600,
  d: 86400,
};

/** Parse a human duration (`3600`, `90m`, `1h`, `7d`) into whole seconds. */
export function parseDuration(text: string): number {
  const match = DURATION.exec(text.trim());
  if (!match) {
    throw new UsageError(`invalid --expiry '${text}' (e.g. 3600, 90m, 1h, 7d)`);
  }
  const seconds = Number(match[1]) * (UNIT_SECONDS[match[2] ?? ""] ?? 1);
  if (!Number.isFinite(seconds) || seconds <= 0) {
    throw new UsageError("--expiry must be greater than zero");
  }
  return seconds;
}

/** Parse the `token mint` flags (tokens after `token mint`). */
export function parseTokenMintOptions(tokens: string[]): TokenMintOptions {
  const grants: string[] = [];
  let expiresInSeconds: number | undefined;
  let account: string | undefined;
  let mailbox: string | undefined;
  let message: string | undefined;

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
    if (name === "--grant") {
      for (const g of value().split(",")) {
        const trimmed = g.trim();
        if (trimmed) grants.push(trimmed);
      }
    } else if (name === "--expiry") expiresInSeconds = parseDuration(value());
    else if (name === "--account") account = value();
    else if (name === "--mailbox") mailbox = value();
    else if (name === "--message") message = value();
    else throw new UsageError(`unknown flag ${name} for 'token mint'`);
  }

  if (grants.length === 0) {
    throw new UsageError(
      "token mint requires at least one --grant (e.g. --grant tap:read,apply,read)",
    );
  }
  const actions = [...new Set(grants.flatMap(grantToActions))];
  return { actions, expiresInSeconds, account, mailbox, message };
}

/** Call the server mint route to attenuate the connection's token. */
export async function mintToken(
  conn: Connection,
  opts: TokenMintOptions,
): Promise<MintedToken> {
  const body: Record<string, unknown> = { actions: opts.actions };
  if (opts.expiresInSeconds !== undefined) {
    body.expiresInSeconds = opts.expiresInSeconds;
  }
  if (opts.account) body.account = opts.account;
  if (opts.mailbox) body.mailbox = opts.mailbox;
  if (opts.message) body.message = opts.message;
  return apiFetch<MintedToken>(conn, "/auth/tokens", { method: "POST", body });
}
