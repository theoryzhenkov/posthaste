import { homedir } from "node:os";
import { join } from "node:path";
import { readFileSync } from "node:fs";

import type { components } from "./schema.gen.js";

/** The typed error body returned by every `/v1` error path. */
export type ApiErrorBody = components["schemas"]["ApiErrorBody"];

/**
 * A resolved connection to a running Posthaste daemon: the `/v1` base URL and
 * an optional bearer token. The token is omitted only when neither the env nor
 * the port-file provided one (the server may have `require_auth` disabled).
 */
export interface Connection {
  /** Base URL including the `/v1` prefix, e.g. `http://127.0.0.1:3001/v1`. */
  baseUrl: string;
  token: string | undefined;
  /** Where the connection was resolved from, for diagnostics. */
  source: "env" | "daemon.json";
}

/** Shape of the `daemon.json` port-file written by the daemon. */
interface PortFile {
  port: number;
  token: string;
}

/**
 * An error raised when the daemon connection cannot be resolved. Carries an
 * actionable message telling the operator to start `posthaste serve`.
 */
export class ConnectionError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "ConnectionError";
  }
}

/**
 * An error raised when the API returns a non-2xx response. Surfaces the typed
 * `code` and `message` from the `ApiErrorBody` when present.
 */
export class ApiError extends Error {
  readonly status: number;
  readonly code: string | undefined;

  constructor(status: number, code: string | undefined, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
  }
}

/**
 * Resolve the state root the daemon writes `daemon.json` into.
 *
 * Mirrors `crates/posthaste-server/src/config.rs`:
 *   - `POSTHASTE_STATE_ROOT` if set, else
 *   - `$XDG_DATA_HOME/posthaste`, else
 *   - `~/.local/share/posthaste`.
 *
 * NOTE: config.rs uses `XDG_DATA_HOME` (not `XDG_STATE_HOME`) and applies the
 * same XDG fallback on every platform, including macOS — there is no
 * `Application Support` special-case. The app dir name is `posthaste`.
 */
function defaultStateRoot(): string {
  const explicit = process.env.POSTHASTE_STATE_ROOT;
  if (explicit && explicit.length > 0) {
    return explicit;
  }
  const xdgData = process.env.XDG_DATA_HOME;
  if (xdgData && xdgData.length > 0) {
    return join(xdgData, "posthaste");
  }
  return join(homedir(), ".local", "share", "posthaste");
}

/** Read and parse `<state_root>/daemon.json`, or return undefined if absent. */
function readPortFile(): PortFile | undefined {
  const path = join(defaultStateRoot(), "daemon.json");
  let raw: string;
  try {
    raw = readFileSync(path, "utf8");
  } catch {
    return undefined;
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new ConnectionError(
      `daemon.json at ${path} is not valid JSON. Restart 'posthaste serve' to rewrite it.`,
    );
  }
  if (
    typeof parsed !== "object" ||
    parsed === null ||
    typeof (parsed as PortFile).port !== "number" ||
    typeof (parsed as PortFile).token !== "string"
  ) {
    throw new ConnectionError(
      `daemon.json at ${path} is missing 'port' or 'token'. Restart 'posthaste serve' to rewrite it.`,
    );
  }
  return parsed as PortFile;
}

/**
 * Resolve the daemon connection. Resolution order:
 *   1. `POSTHASTE_API_URL` / `POSTHASTE_TOKEN` env vars.
 *   2. The daemon port-file `<state_root>/daemon.json`.
 *
 * If `POSTHASTE_API_URL` is set, it is used verbatim as the base (the caller is
 * expected to include the `/v1` prefix). If only the port-file is found, the
 * base is `http://127.0.0.1:<port>/v1`.
 */
export function resolveConnection(): Connection {
  const envUrl = process.env.POSTHASTE_API_URL;
  const envToken = process.env.POSTHASTE_TOKEN;
  if (envUrl && envUrl.length > 0) {
    return {
      baseUrl: envUrl.replace(/\/+$/, ""),
      token: envToken && envToken.length > 0 ? envToken : undefined,
      source: "env",
    };
  }

  const portFile = readPortFile();
  if (!portFile) {
    throw new ConnectionError(
      "Could not find a running Posthaste daemon. Start it with 'posthaste serve', " +
        "or set POSTHASTE_API_URL (and POSTHASTE_TOKEN) to point at one. " +
        `Looked for daemon.json under ${defaultStateRoot()}.`,
    );
  }
  return {
    baseUrl: `http://127.0.0.1:${portFile.port}/v1`,
    token: portFile.token,
    source: "daemon.json",
  };
}

/** Options for a single API request. */
export interface ApiFetchOptions {
  /** HTTP method; defaults to GET. */
  method?: "GET" | "POST";
  /** Query parameters; undefined/null values are omitted. */
  query?: Record<string, string | number | undefined | null>;
  /** JSON request body for POST operations. */
  body?: unknown;
}

/**
 * A typed fetch against the daemon `/v1` API. Sends `Authorization: Bearer`
 * when a token is resolved (works whether or not the server enforces auth),
 * builds the query string, and parses a typed `ApiErrorBody` on non-2xx,
 * throwing an `ApiError` carrying the `code` + `message`.
 *
 * `path` is appended to the base URL and must NOT include the `/v1` prefix
 * (the base already carries it), e.g. `/accounts`, `/sources/x/messages/y`.
 */
export async function apiFetch<T>(
  conn: Connection,
  path: string,
  opts: ApiFetchOptions = {},
): Promise<T> {
  const url = new URL(conn.baseUrl + path);
  if (opts.query) {
    for (const [key, value] of Object.entries(opts.query)) {
      if (value !== undefined && value !== null) {
        url.searchParams.set(key, String(value));
      }
    }
  }

  const headers: Record<string, string> = { accept: "application/json" };
  if (conn.token) {
    headers.authorization = `Bearer ${conn.token}`;
  }
  const method = opts.method ?? "GET";
  let bodyText: string | undefined;
  if (opts.body !== undefined) {
    headers["content-type"] = "application/json";
    bodyText = JSON.stringify(opts.body);
  }

  let res: Response;
  try {
    res = await fetch(url, { method, headers, body: bodyText });
  } catch (cause) {
    throw new ApiError(
      0,
      undefined,
      `request to ${url.toString()} failed: ${cause instanceof Error ? cause.message : String(cause)}`,
    );
  }

  const text = await res.text();
  if (!res.ok) {
    let parsed: Partial<ApiErrorBody> | undefined;
    try {
      parsed = text ? (JSON.parse(text) as ApiErrorBody) : undefined;
    } catch {
      parsed = undefined;
    }
    const code = parsed?.code;
    const message = parsed?.message ?? text ?? res.statusText;
    throw new ApiError(
      res.status,
      code,
      `API ${res.status}${code ? ` [${code}]` : ""}: ${message}`,
    );
  }

  if (!text) {
    return undefined as T;
  }
  return JSON.parse(text) as T;
}
