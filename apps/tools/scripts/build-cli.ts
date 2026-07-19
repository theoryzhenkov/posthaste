// Compile the standalone posthastectl binary (Bun `--compile`).
//
// One invocation for both the host build (`bun run build:cli`) and the
// cross-compile targets a release pipeline consumes, so the flags cannot
// drift between them.
//
// Usage (from the repo root or the package):
//   bun run apps/tools/scripts/build-cli.ts                    # host platform
//   bun run apps/tools/scripts/build-cli.ts bun-darwin-arm64   # cross-compile
//   bun run apps/tools/scripts/build-cli.ts bun-windows-x64 path/to/out
//
// Contract: the FINAL artifact path (incl. ".exe" on windows) is printed as
// the LAST line of stdout so CI can capture it. Exit 0 on success, 2 on an
// unsupported target, non-zero on build failure.

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

// `apps/tools` — entry + default outfile resolve against it (cwd-agnostic).
const PKG_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const ENTRY = join(PKG_ROOT, "src", "cli.ts");

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
  : join(PKG_ROOT, "dist", `posthastectl${short ? `-${short}` : ""}`);

await mkdir(dirname(outfile), { recursive: true });

const args = ["build", "--compile", "--minify"];
// Stamp the release channel so the release smoke's --print-release-channel
// convention works for the CLI too. Empty outside release builds.
const channel = process.env.POSTHASTE_RELEASE_CHANNEL ?? "";
args.push(`--define=POSTHASTE_BUILD_CHANNEL=${JSON.stringify(channel)}`);
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
