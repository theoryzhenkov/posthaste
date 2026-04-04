import pino from "pino";
import { invoke } from "@tauri-apps/api/core";

function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Map pino numeric levels to tracing level names. */
const LEVEL_NAMES: Record<number, string> = {
  10: "trace",
  20: "debug",
  30: "info",
  40: "warn",
  50: "error",
  60: "error",
};

type WriteObj = Record<string, unknown> & { level: number; domain?: string; msg?: string };

/**
 * Forward a pino log object to the Rust tracing subscriber via Tauri IPC.
 * Fire-and-forget — logging should never block the UI.
 */
function sendToBackend(obj: WriteObj): void {
  const level = LEVEL_NAMES[obj.level] ?? "info";
  const domain = obj.domain ?? "app";
  const message = obj.msg ?? JSON.stringify(obj);
  invoke("log_from_frontend", { level, domain, message }).catch(() => {});
}

/**
 * Build a pino `browser.write` handler that forwards logs to the Rust backend
 * via Tauri IPC while also writing to the browser console for dev convenience.
 */
function makeTauriWrite(): pino.LoggerOptions["browser"] {
  const write = (obj: object) => {
    sendToBackend(obj as WriteObj);
  };
  return {
    // Keep console output in dev for devtools, skip in production.
    asObject: true,
    write: import.meta.env.DEV
      ? {
          trace: (obj: object) => { console.debug(obj); write(obj); },
          debug: (obj: object) => { console.debug(obj); write(obj); },
          info: (obj: object) => { console.info(obj); write(obj); },
          warn: (obj: object) => { console.warn(obj); write(obj); },
          error: (obj: object) => { console.error(obj); write(obj); },
          fatal: (obj: object) => { console.error(obj); write(obj); },
        }
      : write,
  };
}

const browserOpts: pino.LoggerOptions["browser"] = isTauri()
  ? makeTauriWrite()
  : { asObject: true };

const logger = pino({
  level: import.meta.env.DEV ? "debug" : "info",
  browser: browserOpts,
});

export const syncLogger = logger.child({ domain: "sync" });
export const uiLogger = logger.child({ domain: "ui" });
export const apiLogger = logger.child({ domain: "api" });

export default logger;
