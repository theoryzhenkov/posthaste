import { defineConfig } from "@playwright/test";
import type { TestMode } from "@srsholmes/tauri-playwright";
import { env } from "node:process";

const runDir = env.POSTHASTE_LAB_RUN_DIR?.trim();
if (!runDir) {
  throw new Error(
    "POSTHASTE_LAB_RUN_DIR must be set by tools/lab/tauri-playwright/smoke.sh.",
  );
}

interface TauriProjectOptions {
  mode: TestMode;
}

export default defineConfig<TauriProjectOptions>({
  testDir: ".",
  testMatch: ["main-window.spec.ts"],
  fullyParallel: false,
  workers: 1,
  timeout: 120_000,
  outputDir: `${runDir}/artifacts/playwright-output`,
  reporter: [
    ["list"],
    ["json", { outputFile: `${runDir}/artifacts/playwright-results.json` }],
  ],
  projects: [
    {
      name: "tauri-main",
      use: {
        mode: "tauri",
      },
    },
  ],
});
