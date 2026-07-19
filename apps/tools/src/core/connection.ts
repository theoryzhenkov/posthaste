// Connection discovery: where the integrated backend listens and the session
// secret to present. The backend writes `connection-info.json` ({port, token})
// into its state root; possession of that file is the local trust boundary.
//
// Resolution precedence: explicit overrides (--base-url) > environment
// (POSTHASTE_API_URL / POSTHASTE_TOKEN) > the connection-info file under the
// state root (POSTHASTE_STATE_ROOT > $XDG_DATA_HOME/posthaste >
// ~/.local/share/posthaste — the same resolver the backend uses).
//
// The token deliberately has no CLI flag: secrets never travel on argv. It
// comes from the environment or the owner-only discovery file.

import { homedir } from "node:os";
import { join } from "node:path";
import { readFileSync } from "node:fs";

/** A resolved connection: origin (no trailing slash) plus the bearer token. */
export interface Connection {
  /** Base origin, e.g. `http://127.0.0.1:49152` — endpoints append `/query` etc. */
  baseUrl: string;
  token: string | undefined;
  /** Where the connection was resolved from, for diagnostics. */
  source: "env" | "connection-info" | "flag";
  /**
   * Injectable fetch so every layer above (client, events, CLI, MCP) is
   * testable without sockets. Defaults to the global `fetch`.
   */
  fetch?: typeof fetch;
}

/** Explicit overrides (the CLI's `--base-url`). */
export interface ConnectionOverrides {
  baseUrl?: string;
}

/** The connection cannot be resolved — actionable, never carries the token. */
export class ConnectionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ConnectionError";
  }
}

/** Shape of the backend's `connection-info.json`; extra fields tolerated. */
interface ConnectionInfoFile {
  port: number;
  token: string;
}

/** Env access, injectable for tests. */
export type Env = Record<string, string | undefined>;

function envOrUndefined(value: string | undefined): string | undefined {
  return value && value.length > 0 ? value : undefined;
}

/** State root: `$POSTHASTE_STATE_ROOT` > `$XDG_DATA_HOME/posthaste` > `~/.local/share/posthaste`. */
export function stateRoot(env: Env): string {
  const explicit = envOrUndefined(env.POSTHASTE_STATE_ROOT);
  if (explicit) return explicit;
  const xdgData = envOrUndefined(env.XDG_DATA_HOME);
  if (xdgData) return join(xdgData, "posthaste");
  return join(homedir(), ".local", "share", "posthaste");
}

/** Path of the backend's connection-info file. */
export function connectionInfoPath(env: Env): string {
  return join(stateRoot(env), "connection-info.json");
}

/** Read and validate the connection-info file; undefined when absent. */
function readConnectionInfo(
  env: Env,
  readFile: (path: string) => string,
): ConnectionInfoFile | undefined {
  const path = connectionInfoPath(env);
  let raw: string;
  try {
    raw = readFile(path);
  } catch {
    return undefined;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new ConnectionError(
      `connection info at ${path} is not valid JSON; restart the Posthaste app to rewrite it`,
    );
  }
  if (
    typeof parsed !== "object" ||
    parsed === null ||
    typeof (parsed as ConnectionInfoFile).port !== "number" ||
    typeof (parsed as ConnectionInfoFile).token !== "string"
  ) {
    throw new ConnectionError(
      `connection info at ${path} is missing 'port' or 'token'; restart the Posthaste app to rewrite it`,
    );
  }
  return parsed as ConnectionInfoFile;
}

/** Injectable inputs for [`resolveConnection`], defaulted for production. */
export interface ResolveDeps {
  env?: Env;
  readFile?: (path: string) => string;
}

/**
 * Resolve the backend connection: overrides > env > connection-info file.
 * Throws [`ConnectionError`] when nothing is found.
 */
export function resolveConnection(
  overrides: ConnectionOverrides = {},
  deps: ResolveDeps = {},
): Connection {
  const env = deps.env ?? process.env;
  const readFile = deps.readFile ?? ((path) => readFileSync(path, "utf8"));

  const envToken = envOrUndefined(env.POSTHASTE_TOKEN);

  if (overrides.baseUrl && overrides.baseUrl.length > 0) {
    return {
      baseUrl: overrides.baseUrl.replace(/\/+$/, ""),
      // A flag-supplied URL still takes its token from the environment or the
      // discovery file (secrets never on argv).
      token: envToken ?? tokenFromFile(env, readFile),
      source: "flag",
    };
  }

  const envUrl = envOrUndefined(env.POSTHASTE_API_URL);
  if (envUrl) {
    return {
      baseUrl: envUrl.replace(/\/+$/, ""),
      token: envToken,
      source: "env",
    };
  }

  const info = readConnectionInfo(env, readFile);
  if (!info) {
    throw new ConnectionError(
      "could not find a running Posthaste backend: no connection info at " +
        `${connectionInfoPath(env)} and POSTHASTE_API_URL is unset. ` +
        "Start the Posthaste app, or point POSTHASTE_API_URL (and POSTHASTE_TOKEN) at a backend.",
    );
  }
  return {
    baseUrl: `http://127.0.0.1:${info.port}`,
    token: envToken ?? info.token,
    source: "connection-info",
  };
}

/** Best-effort token from the discovery file (for the base-url override path). */
function tokenFromFile(
  env: Env,
  readFile: (path: string) => string,
): string | undefined {
  try {
    return readConnectionInfo(env, readFile)?.token;
  } catch {
    return undefined;
  }
}
