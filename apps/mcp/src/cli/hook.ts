/**
 * `posthastectl hook serve --exec <script>` — the built-in localhost webhook
 * receiver (RFC-L2-scripting ruling 17): the easy listener for a GUI/`rules.toml`
 * `webhook` action, so a user doesn't have to stand up their own HTTP server to
 * pair with `emit`/`webhook` rules. One delivery, one `--exec` invocation — the
 * same "payload on stdin, common fields as PH_* env" contract as `watch --exec`
 * and the Level-1 `exec` rule action
 * (`crates/posthaste-authority-server/src/rules/actions.rs::exec_env_vars`),
 * so a handler written for one works for the other.
 *
 * PAYLOAD-IS-DATA (ruling 20a): the POST body is delivered to the script
 * verbatim on **stdin** as JSON — never parsed into a shell command, never
 * placed on argv. A malicious/malformed body cannot inject a command; at worst
 * a bad JSON parse leaves the derived PH_* env vars empty while stdin still
 * carries the raw bytes untouched.
 *
 * Binds `127.0.0.1` only (ruling 15: localhost-first) — there is no `--host`
 * flag on purpose; a rule targeting this receiver over the network is not a
 * shape this command supports.
 *
 * @spec docs/eph/RFC-L2-scripting.md ruling 17, ruling 19, ruling 20
 */

/** Options for `hook serve`. */
export interface HookServeOptions {
  /** Shell command run per delivery (POST body on stdin + PH_* env). */
  exec: string;
  /** Port to listen on; `0` lets the OS assign one. Default 8787. */
  port?: number;
  /** URL path deliveries must POST to. Default `/hook`. */
  path?: string;
  /**
   * Require `Authorization: Bearer <token>` on every delivery — so only a rule
   * that knows the token (configured as a header on the webhook action, or
   * embedded in the URL by the operator) can invoke this receiver. Optional:
   * without it, anything that can reach 127.0.0.1 on this port can POST.
   */
  token?: string;
}

/** Injectable side-effects, mirroring `watch`'s `WatchDeps` shape. */
export interface HookServeDeps {
  /** Run the handler; resolves with the exit code (never rejects). */
  runCommand: (
    command: string,
    input: string,
    env: Record<string, string>,
  ) => Promise<number>;
  /** Diagnostics on stderr. */
  log: (line: string) => void;
  /**
   * The daemon's `/v1` base URL, if discoverable at startup — exported to the
   * handler as `POSTHASTE_API_URL` so the "posthastectl IS the SDK" write
   * verbs (`tag`/`move`/`reply`/`send`/`apply`) work with no further setup.
   * Best-effort: `undefined` when no daemon was discovered (the handler can
   * still act using the per-delivery token via raw REST, or the caller can
   * export `POSTHASTE_API_URL` itself).
   */
  apiUrl?: string;
}

/** A running hook receiver. */
export interface HookServerHandle {
  /** The bound port (resolved even when `--port 0` asked for an OS-assigned one). */
  port: number;
  /** The full URL deliveries should POST to. */
  url: string;
  /** Stop listening immediately (aborts in-flight connections). */
  stop: () => void;
}

const DEFAULT_HOOK_PORT = 8787;
const DEFAULT_HOOK_PATH = "/hook";

function normalizePath(path: string): string {
  return path.startsWith("/") ? path : `/${path}`;
}

/**
 * The shape of a Level-1 hook delivery (webhook action payload — see
 * `run_hook` in `crates/posthaste-authority-server/src/rules/actions.rs`):
 * `{ ruleId, idempotencyKey, event: {seq,topic,accountId,mailboxId,messageId},
 * message: <MessageSummary>, token }`. Every field is optional here — the
 * receiver never trusts the shape, only forwards it (payload-is-data).
 */
interface HookPayloadish {
  ruleId?: unknown;
  idempotencyKey?: unknown;
  event?: {
    seq?: unknown;
    topic?: unknown;
    accountId?: unknown;
    messageId?: unknown;
  };
  message?: {
    fromEmail?: unknown;
    fromName?: unknown;
    subject?: unknown;
    keywords?: unknown;
  };
  token?: unknown;
}

function asString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((v): v is string => typeof v === "string")
    : [];
}

/**
 * Derive the `PH_*` env vars from a raw delivery body. Mirrors the Level-1
 * `exec` action's `exec_env_vars` set exactly (`PH_IDEMPOTENCY_KEY`,
 * `PH_ACCOUNT`, `PH_MESSAGE_ID`, `PH_FROM`, `PH_SUBJECT`, `PH_KEYWORDS`,
 * `PH_EVENT_SEQ`, `PH_TOPIC`), adds `PH_RULE` (the rule filter's counterpart
 * on the `watch --rule` side, ruling 19), and threads the payload's
 * per-invocation `token` into `POSTHASTE_TOKEN` plus the discovered
 * `apiUrl` into `POSTHASTE_API_URL` — together the write verbs need nothing
 * else to act. Never throws: an unparseable body yields an all-empty env
 * (the raw bytes are still delivered on stdin untouched).
 */
export function deriveHookEnv(
  raw: string,
  apiUrl: string | undefined,
): Record<string, string> {
  let payload: HookPayloadish = {};
  try {
    const parsed: unknown = JSON.parse(raw);
    if (typeof parsed === "object" && parsed !== null) {
      payload = parsed as HookPayloadish;
    }
  } catch {
    /* malformed body: PH_* stay empty, stdin still carries the raw bytes */
  }

  const event = payload.event ?? {};
  const message = payload.message ?? {};
  const seq = typeof event.seq === "number" ? event.seq : undefined;
  const from = asString(message.fromEmail) ?? asString(message.fromName) ?? "";

  const env: Record<string, string> = {
    PH_RULE: asString(payload.ruleId) ?? "",
    PH_IDEMPOTENCY_KEY: asString(payload.idempotencyKey) ?? "",
    PH_ACCOUNT: asString(event.accountId) ?? "",
    PH_MESSAGE_ID: asString(event.messageId) ?? "",
    PH_FROM: from,
    PH_SUBJECT: asString(message.subject) ?? "",
    PH_KEYWORDS: asStringArray(message.keywords).join(","),
    PH_EVENT_SEQ: seq !== undefined ? String(seq) : "",
    PH_TOPIC: asString(event.topic) ?? "",
  };
  const token = asString(payload.token);
  if (token) env.POSTHASTE_TOKEN = token;
  if (apiUrl) env.POSTHASTE_API_URL = apiUrl;
  return env;
}

/**
 * Start the hook receiver (`Bun.serve`, no new dependency). Binds
 * `127.0.0.1:<port>` (default {@link DEFAULT_HOOK_PORT}) and answers `POST
 * <path>` (default {@link DEFAULT_HOOK_PATH}) only — everything else is `404`.
 * When `opts.token` is set, a missing/mismatched `Authorization: Bearer`
 * header is `401` and the handler never runs (ruling 20a note: this token is
 * still a convenience gate on *who may invoke the receiver*, not a substitute
 * for the per-delivery capability token already inside the payload).
 *
 * Each accepted delivery reads the full body, derives the `PH_*`/
 * `POSTHASTE_TOKEN`/`POSTHASTE_API_URL` env (see {@link deriveHookEnv}), and
 * runs `opts.exec` with the **raw body bytes** on stdin — never
 * re-serialized, so a payload survives verbatim even if it were not valid
 * JSON. Responds `200` on a zero exit, `500` (logged) otherwise.
 */
export function startHookServer(
  opts: HookServeOptions,
  deps: HookServeDeps,
): HookServerHandle {
  const path = normalizePath(opts.path ?? DEFAULT_HOOK_PATH);
  const expectedAuth = opts.token ? `Bearer ${opts.token}` : undefined;

  const server = Bun.serve({
    hostname: "127.0.0.1",
    port: opts.port ?? DEFAULT_HOOK_PORT,
    fetch: async (req) => {
      const url = new URL(req.url);
      if (req.method !== "POST" || url.pathname !== path) {
        return new Response("not found", { status: 404 });
      }
      if (expectedAuth && req.headers.get("authorization") !== expectedAuth) {
        deps.log(`rejected delivery to ${path}: missing or wrong bearer token`);
        return new Response("unauthorized", { status: 401 });
      }

      const body = await req.text();
      const env = deriveHookEnv(body, deps.apiUrl);
      const code = await deps.runCommand(opts.exec, body, env);
      if (code !== 0) {
        deps.log(`--exec exited ${code} for a delivery on ${path}`);
        return new Response("handler failed", { status: 500 });
      }
      return new Response("ok", { status: 200 });
    },
  });

  const port = server.port ?? opts.port ?? DEFAULT_HOOK_PORT;
  const url = `http://127.0.0.1:${port}${path}`;
  deps.log(`hook serve: listening on ${url}`);
  return { port, url, stop: () => server.stop(true) };
}
