import { createTauriTest } from "@srsholmes/tauri-playwright";
import { isAbsolute, relative, resolve } from "node:path";
import { cwd, env } from "node:process";

const DEFAULT_TAURI_PLAYWRIGHT_SOCKET = "/tmp/tauri-playwright.sock";

function requiredPrivateSocket(): string {
  const socket = env.POSTHASTE_E2E_SOCKET?.trim();
  if (!socket) {
    throw new Error(
      "POSTHASTE_E2E_SOCKET must point at a private per-run Unix socket for Lab Tauri smoke tests.",
    );
  }
  if (socket === DEFAULT_TAURI_PLAYWRIGHT_SOCKET) {
    throw new Error(
      `POSTHASTE_E2E_SOCKET must not use the tauri-playwright default ${DEFAULT_TAURI_PLAYWRIGHT_SOCKET}.`,
    );
  }

  const runDir = env.POSTHASTE_LAB_RUN_DIR?.trim();
  if (runDir) {
    const relativeSocket = relative(resolve(runDir), resolve(socket));
    if (
      relativeSocket === "" ||
      relativeSocket.startsWith("..") ||
      isAbsolute(relativeSocket)
    ) {
      throw new Error(
        "POSTHASTE_E2E_SOCKET must be inside POSTHASTE_LAB_RUN_DIR for Lab Tauri smoke tests.",
      );
    }
  }

  return socket;
}

function startTimeoutSeconds(): number {
  const rawTimeout = env.POSTHASTE_E2E_START_TIMEOUT_SECONDS?.trim();
  if (!rawTimeout) {
    return 180;
  }
  const parsed = Number(rawTimeout);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(
      `POSTHASTE_E2E_START_TIMEOUT_SECONDS must be a positive number, got ${rawTimeout}.`,
    );
  }
  return parsed;
}

export const { test, expect } = createTauriTest({
  devUrl: "",
  mcpSocket: requiredPrivateSocket(),
  tauriCommand: "bun run tauri dev --config legacy/desktop/tauri.e2e.conf.json",
  tauriCwd: env.POSTHASTE_REPO_ROOT ?? cwd(),
  tauriFeatures: ["e2e-testing"],
  startTimeout: startTimeoutSeconds(),
});
