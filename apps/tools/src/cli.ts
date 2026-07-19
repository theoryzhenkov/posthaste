#!/usr/bin/env bun
// posthastectl entry: wire real side-effects into the pure dispatcher.
// `posthastectl mcp` starts the stdio MCP server from the same binary, so
// one compiled sidecar serves both scripts and agent hosts.

import { spawn } from "node:child_process";
import { readFile } from "node:fs/promises";

import { resolveConnection } from "./core/connection.js";
import { operations } from "./operations/index.js";
import { run, type RunDeps } from "./cli/run.js";

/** posthastectl version (also the MCP server version). */
const CLI_VERSION = "0.1.0";

/** Read all of stdin to a string (for `--input -`). */
async function readStdin(): Promise<string> {
  const chunks: Buffer[] = [];
  for await (const chunk of process.stdin) {
    chunks.push(chunk as Buffer);
  }
  return Buffer.concat(chunks).toString("utf8");
}

/**
 * Run a `watch --exec` command via the shell with `input` on stdin and `env`
 * merged in. The child inherits stdout/stderr so its output is visible.
 * Resolves with the exit code; a spawn failure resolves to 127 (never
 * rejects), so a bad command logs rather than crashing the watcher.
 */
function runCommand(
  command: string,
  input: string,
  env: Record<string, string>,
): Promise<number> {
  return new Promise((resolve) => {
    const child = spawn(command, {
      shell: true,
      env: { ...process.env, ...env },
      stdio: ["pipe", "inherit", "inherit"],
    });
    child.on("error", (error) => {
      process.stderr.write(
        `posthastectl: failed to run --exec command: ${error.message}\n`,
      );
      resolve(127);
    });
    child.on("close", (code) => resolve(code ?? 0));
    if (child.stdin) {
      child.stdin.on("error", () => {
        /* child closed stdin early; ignore EPIPE */
      });
      child.stdin.end(input);
    }
  });
}

async function main(): Promise<void> {
  // `posthastectl mcp` starts the stdio MCP server (this package's other
  // front-end). Args after `mcp` are ignored.
  if (process.argv[2] === "mcp") {
    const { main: mcpMain } = await import("./mcp.js");
    await mcpMain();
    return;
  }

  // Ctrl-C cleanly ends the `events`/`watch` stream.
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
    runCommand,
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
