// The typed error surface: every failed HTTP call carries the generated
// `ApiError` envelope ({kind, message, retryable}); this module wraps it in a
// throwable that preserves the typed fields so callers match on kinds, never
// on message strings. Error text never includes the bearer token.

import type { ApiError, ApiErrorKind } from "@posthaste/protocol/gen";

/** A non-2xx API response, carrying the typed envelope when the body had one. */
export class ApiCallError extends Error {
  readonly status: number;
  /** The typed kind; `undefined` when the body was not a valid envelope. */
  readonly kind: ApiErrorKind | undefined;
  readonly retryable: boolean;

  constructor(status: number, envelope: Partial<ApiError> | undefined, fallback: string) {
    const kind = isKind(envelope?.kind) ? envelope.kind : undefined;
    const message = envelope?.message ?? fallback;
    super(`API ${status}${kind ? ` [${kind}]` : ""}: ${message}`);
    this.name = "ApiCallError";
    this.status = status;
    this.kind = kind;
    this.retryable = envelope?.retryable === true;
  }
}

/** A transport-level failure (connection refused, aborted mid-stream, ...). */
export class TransportError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "TransportError";
  }
}

const KINDS: readonly ApiErrorKind[] = [
  "malformedRequest",
  "unauthorized",
  "capabilityDenied",
  "unknownId",
  "conflict",
  "unavailable",
  "internal",
];

function isKind(value: unknown): value is ApiErrorKind {
  return typeof value === "string" && (KINDS as readonly string[]).includes(value);
}

/** Parse a response body into the typed envelope, or undefined. */
export function parseErrorBody(text: string): Partial<ApiError> | undefined {
  if (!text) return undefined;
  try {
    const parsed = JSON.parse(text) as unknown;
    if (typeof parsed === "object" && parsed !== null) {
      return parsed as Partial<ApiError>;
    }
  } catch {
    /* non-JSON error body */
  }
  return undefined;
}
