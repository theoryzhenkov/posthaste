import type { Connection } from "./client.js";
import { grantsToActions, mintToken, parseDuration } from "./cli/token.js";

/**
 * Connect-time, per-connection token minting — the survey gap in ruling 22.
 * `resolveConnection()` bakes ONE discovered bootstrap token for the whole
 * stdio session; the agent should instead run under a token scoped to exactly
 * what it declared it needs. On startup the MCP server attenuates the bootstrap
 * into a least-grant token (server-side macaroon attenuation via the existing
 * `mintToken` pipeline) and uses it for every tool call and the subscription.
 *
 * Least-grant guidance (scripting-security.md, threat 2): the default is
 * read-only + subscribe. Write verbs are opt-in — an agent that reads untrusted
 * message content AND holds `apply`/`send` is the prompt-injection blast-radius.
 * Grant `apply`/`send` only when you have accepted that surface.
 *
 * @spec docs/eph/RFC-L2-scripting.md ruling 22
 */

/** The default grant set when the host declares none: read-only + tap subscribe. */
export const DEFAULT_MCP_GRANTS = "tap:read,read";

/**
 * Resolve the comma-separated grant declaration (env/arg) into the deduped wire
 * action verbs. An empty/absent declaration falls back to [`DEFAULT_MCP_GRANTS`]
 * (read-only + subscribe) — a write grant is always an explicit opt-in.
 */
export function resolveConnectGrants(raw: string | undefined): string[] {
  const source = raw && raw.trim().length > 0 ? raw : DEFAULT_MCP_GRANTS;
  const grants = source
    .split(",")
    .map((g) => g.trim())
    .filter((g) => g.length > 0);
  return grantsToActions(grants);
}

/** Inputs to the connect-time mint, read from the environment by `index.ts`. */
export interface ConnectMintOptions {
  /** `POSTHASTE_MCP_GRANTS` — the grant declaration (comma list). */
  grants?: string;
  /** `POSTHASTE_MCP_TOKEN_EXPIRY` — a human duration (`1h`, `90m`, `3600`). */
  expiry?: string;
  /** Optional `--account` narrowing of the minted token. */
  account?: string;
}

/** The outcome of a connect-time mint attempt. */
export interface ConnectMintResult {
  /** The connection to run under — carries the minted token when minting ran. */
  conn: Connection;
  /** The resolved wire action verbs the token was scoped to. */
  actions: string[];
  /** Whether a fresh token was minted (false → ran under the bootstrap token). */
  minted: boolean;
  /** A diagnostic line describing what happened (always set). */
  detail: string;
}

/**
 * Mint a per-connection scoped token from the discovered bootstrap and return a
 * connection that uses it. When the bootstrap has no token (an auth-disabled
 * dev daemon) minting is skipped and the original connection is returned. A mint
 * failure is non-fatal: we fall back to the bootstrap connection with a warning,
 * so the server still starts (albeit under a wider token) rather than refusing
 * to connect.
 */
export async function mintConnectionToken(
  conn: Connection,
  opts: ConnectMintOptions,
): Promise<ConnectMintResult> {
  const actions = resolveConnectGrants(opts.grants);

  if (!conn.token) {
    return {
      conn,
      actions,
      minted: false,
      detail:
        "no bootstrap token to attenuate (auth-disabled daemon); running unscoped",
    };
  }

  const expiresInSeconds =
    opts.expiry && opts.expiry.trim().length > 0
      ? parseDuration(opts.expiry)
      : undefined;

  try {
    const minted = await mintToken(conn, {
      actions,
      expiresInSeconds,
      account: opts.account,
    });
    return {
      conn: { ...conn, token: minted.token },
      actions,
      minted: true,
      detail:
        `minted a connection token scoped to [${actions.join(", ")}]` +
        (minted.expiresAt ? `, expires ${minted.expiresAt}` : ""),
    };
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    return {
      conn,
      actions,
      minted: false,
      detail: `mint failed (${message}); falling back to the bootstrap token`,
    };
  }
}
