// Cross-compile the standalone posthastectl binary (Bun `--compile`).
//
// One source of truth for both the host build (`just mcp build-cli`) and the
// cross-compile targets the release pipeline consumes. Routes every build
// through the same `bun build --compile --minify` invocation so the flags can't
// drift between the two.
//
// Usage (from the repo root):
//   bun run apps/mcp/scripts/build-cli.ts                       # host platform
//   bun run apps/mcp/scripts/build-cli.ts bun-darwin-arm64      # cross-compile
//   bun run apps/mcp/scripts/build-cli.ts bun-windows-x64 path/to/out
//
// Contract (for the release job that consumes this):
//   $ just mcp build-cli-target <target> [outfile]
//     <target>  one of the SUPPORTED list below (Bun's `bun-<os>-<arch>` names).
//     [outfile] optional; repo-root-relative or absolute. Defaults to
//               apps/mcp/dist/posthastectl-<os>-<arch>.
//   - Bun appends ".exe" for windows targets; this script resolves the FINAL
//     artifact path (incl. ".exe") and prints it as the LAST line of stdout, so
//     CI can capture it (`... | tail -1`).
//   - Exit 0 on success, 2 on an unsupported target, non-zero on build failure.
//   - Cross-compiling downloads the target's Bun bootstrap (~20-40 MB, cached)
//     and works for all five targets from a single Linux runner.

import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdir } from "node:fs/promises";

const SUPPORTED = [
  "bun-linux-x64",
  "bun-linux-arm64",
  "bun-darwin-x64",
  "bun-darwin-arm64",
  "bun-windows-x64",
] as const;
type Target = (typeof SUPPORTED)[number];

const isSupported = (t: string): t is Target =>
  (SUPPORTED as readonly string[]).includes(t);

// `apps/mcp` (this file is apps/mcp/scripts/build-cli.ts) — entry + default
// outfile resolve against it so the build is cwd-agnostic.
const MCP_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const ENTRY = join(MCP_ROOT, "src", "cli.ts");

const argv = process.argv.slice(2).filter((a) => a !== "--");
const target = argv[0];
const userOutfile = argv[1];

if (target !== undefined && !isSupported(target)) {
  console.error(
    `unsupported target ${JSON.stringify(target)}. supported: ${SUPPORTED.join(", ")}`,
  );
  process.exit(2);
}

const short = target ? target.replace(/^bun-/, "") : null;
const outfile = userOutfile
  ? resolve(process.cwd(), userOutfile)
  : join(MCP_ROOT, "dist", `posthastectl${short ? `-${short}` : ""}`);

await mkdir(dirname(outfile), { recursive: true });

const args = ["build", "--compile", "--minify"];
if (target) args.push(`--target=${target}`);
args.push(`--outfile=${outfile}`, ENTRY);

const proc = Bun.spawn([process.execPath, ...args], {
  stdio: ["inherit", "inherit", "inherit"],
});
const code = await proc.exited;
if (code !== 0) {
  console.error(`bun build exited ${code}`);
  process.exit(code);
}

// Bun appends ".exe" to the outfile for windows targets.
const isWindows = target?.startsWith("bun-windows-") ?? false;
const finalPath =
  isWindows && !outfile.endsWith(".exe") ? `${outfile}.exe` : outfile;
console.log(finalPath);
