// The typed HTTP client over the integration surface: POST /query, POST
// /command, blob GETs. Wire shapes come exclusively from the generated
// protocol package; this module adds transport, auth, and the idempotency-id
// mint. The bearer token travels only in headers — never in URLs, logs, or
// error messages.

import type {
  Command,
  CommandAccepted,
  CommandEnvelope,
  Query,
  QueryEnvelope,
} from "@posthaste/protocol/gen";

import type { Connection } from "./connection.js";
import { ApiCallError, TransportError, parseErrorBody } from "./errors.js";

/** Perform one authenticated JSON POST and parse the response. */
async function postJson<T>(conn: Connection, path: string, body: unknown): Promise<T> {
  const doFetch = conn.fetch ?? fetch;
  const headers: Record<string, string> = {
    accept: "application/json",
    "content-type": "application/json",
  };
  if (conn.token) headers.authorization = `Bearer ${conn.token}`;

  let res: Response;
  try {
    res = await doFetch(`${conn.baseUrl}${path}`, {
      method: "POST",
      headers,
      body: JSON.stringify(body),
    });
  } catch (cause) {
    throw new TransportError(
      `request to ${conn.baseUrl}${path} failed: ${cause instanceof Error ? cause.message : String(cause)}`,
    );
  }

  const text = await res.text();
  if (!res.ok) {
    throw new ApiCallError(res.status, parseErrorBody(text), text || res.statusText);
  }
  return JSON.parse(text) as T;
}

/**
 * Run one typed query. `T` is the family's generated result type
 * (`MailListResult`, `ThreadView`, ...) matching the `Query` variant sent.
 */
export function runQuery<T>(conn: Connection, query: Query): Promise<QueryEnvelope<T>> {
  return postJson<QueryEnvelope<T>>(conn, "/query", query);
}

/**
 * Submit one typed command. Mints a fresh idempotency id unless the caller
 * supplies one (a retry). Acceptance means recorded-and-visible at the
 * returned generation; the provider verdict is pending-operations state.
 */
export async function runCommand(
  conn: Connection,
  command: Command,
  id?: string,
): Promise<CommandAccepted & { id: string }> {
  const envelope: CommandEnvelope = { id: id ?? mintCommandId(), command };
  const accepted = await postJson<CommandAccepted>(conn, "/command", envelope);
  return { ...accepted, id: envelope.id };
}

/** Fetch one immutable blob's bytes (message bodies, attachments). */
export async function fetchBlob(conn: Connection, blobId: string): Promise<Uint8Array> {
  const doFetch = conn.fetch ?? fetch;
  const headers: Record<string, string> = {};
  if (conn.token) headers.authorization = `Bearer ${conn.token}`;
  let res: Response;
  try {
    res = await doFetch(`${conn.baseUrl}/blobs/${encodeURIComponent(blobId)}`, { headers });
  } catch (cause) {
    throw new TransportError(
      `blob fetch failed: ${cause instanceof Error ? cause.message : String(cause)}`,
    );
  }
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new ApiCallError(res.status, parseErrorBody(text), text || res.statusText);
  }
  return new Uint8Array(await res.arrayBuffer());
}

// --- idempotency ids -------------------------------------------------------

const CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/**
 * Mint a ULID: 48-bit millisecond timestamp + 80 bits of CSPRNG randomness,
 * Crockford base32. Lexically sortable, so the outbox lists commands in mint
 * order, and unique enough that a retry with the SAME id is always deliberate.
 */
export function mintCommandId(now = Date.now()): string {
  let time = "";
  let t = now;
  for (let i = 0; i < 10; i++) {
    time = CROCKFORD[t % 32] + time;
    t = Math.floor(t / 32);
  }
  const rand = new Uint8Array(16);
  crypto.getRandomValues(rand);
  let random = "";
  for (let i = 0; i < 16; i++) {
    random += CROCKFORD[(rand[i] as number) % 32];
  }
  return time + random;
}
