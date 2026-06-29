#!/usr/bin/env bun
import { readFile } from "node:fs/promises";

import { resolveConnection } from "./client.js";
import { operations } from "./operations/index.js";
import { run, type RunDeps } from "./cli/run.js";

/** posthastectl version (independent of the MCP package version). */
const CLI_VERSION = "0.1.0";

/** Read all of stdin to a string (for `--input -`). */
async function readStdin(): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk as Buffer);
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function main(): Promise<void> {
  // Ctrl-C cleanly ends the `events` stream (and any in-flight request).
  const controller = new AbortController();
  process.on("SIGINT", () => controller.abort());

  const deps: RunDeps = {
    operations,
    resolveConnection,
    stdout: (text) => process.stdout.write(text),
    stderr: (text) => process.stderr.write(text),
    isTty: Boolean(process.stdout.isTTY),
    env: process.env,
    readStdin,
    readFile: (path) => readFile(path, "utf8"),
    fetch: globalThis.fetch,
    version: CLI_VERSION,
    signal: controller.signal,
  };

  const code = await run(process.argv.slice(2), deps);
  process.exit(code);
}

if (import.meta.main) {
  main().catch((error) => {
    process.stderr.write(`posthastectl: fatal: ${String(error)}\n`);
    process.exit(1);
  });
}

export { main };
