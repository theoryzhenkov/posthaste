/**
 * Capture WebKit console output and forward it to the Rust tracing subscriber
 * via Tauri IPC. This catches logs from React, third-party libraries, and
 * uncaught errors that bypass Pino.
 *
 * Call `installConsoleCapture()` once at app startup, only in Tauri context.
 */
import { invoke } from "@tauri-apps/api/core";

const CONSOLE_LEVEL_MAP: Record<string, string> = {
  log: "info",
  info: "info",
  debug: "debug",
  warn: "warn",
  error: "error",
};

/** Pino log objects have a numeric `level` field — skip them to avoid
 *  double-sending (pino's write handler already forwards these). */
function isPinoObject(arg: unknown): boolean {
  return typeof arg === "object" && arg !== null && typeof (arg as Record<string, unknown>).level === "number";
}

function formatArgs(args: unknown[]): string {
  return args
    .map((a) => {
      if (typeof a === "string") return a;
      try { return JSON.stringify(a); }
      catch { return String(a); }
    })
    .join(" ");
}

export function installConsoleCapture(): void {
  for (const [method, level] of Object.entries(CONSOLE_LEVEL_MAP)) {
    const original = (console as Record<string, unknown>)[method] as (...args: unknown[]) => void;
    (console as Record<string, unknown>)[method] = (...args: unknown[]) => {
      original.apply(console, args);
      if (args.length === 1 && isPinoObject(args[0])) return;
      invoke("log_from_frontend", {
        level,
        domain: "webview",
        message: formatArgs(args),
      }).catch(() => {});
    };
  }

  window.addEventListener("error", (event) => {
    invoke("log_from_frontend", {
      level: "error",
      domain: "webview",
      message: `Uncaught ${event.error?.stack ?? event.message}`,
    }).catch(() => {});
  });

  window.addEventListener("unhandledrejection", (event) => {
    const reason = event.reason;
    const message =
      reason instanceof Error
        ? reason.stack ?? reason.message
        : String(reason);
    invoke("log_from_frontend", {
      level: "error",
      domain: "webview",
      message: `Unhandled rejection: ${message}`,
    }).catch(() => {});
  });
}
