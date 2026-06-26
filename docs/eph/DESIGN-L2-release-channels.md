---
scope: L2
summary: "Release channels as a first-class product concept: channel is declared (tag-inferred for push, explicit for manual dispatch), baked into the binary, and drives a single policy table covering distinct per-channel identity, updater manifest, devtools, macOS signing, and smoke gates."
modified: 2026-06-26
reviewed: 2026-06-26
lifecycle: ephemeral
type: DESIGN
depends:
  - path: .github/workflows/release
  - path: tools/release/channel-policy
  - path: tools/release/resolve-channel
  - path: tools/release/generate-updater-manifest
  - path: tools/release/smoke-desktop-bundle
  - path: tools/release/update-rolling-tag
  - path: apps/desktop/tauri.conf
  - path: apps/desktop/src/lib
dependents: []
---

# Release channel design

## Principle

The channel is a **first-class concept the product carries**, not a side-effect
of a tag string inferred once in CI. Three things follow from that:

1. **Declared, not just inferred.** Tag-push infers the channel from the tag for
   automation; manual dispatch declares it explicitly. Both routes produce the
   same single output: `channel`.
2. **Baked into the binary.** The desktop binary embeds its channel as a
   compile-time constant and a sentinel string. The renderer receives the same
   channel via `VITE_RELEASE_CHANNEL`. An artifact cannot silently be on the
   wrong channel — the smoke step proves it.
3. **Policy flows from the channel.** One committed policy table maps a channel
   to its identity, manifest, devtools flag, signing policy, and updater
   endpoint. Jobs read the table by channel; they do not thread six booleans.

## Channels

```text
┌─────────┬────────────────┬──────────────────────────────┬───────────────────────┐
│ Channel │ Audience       │ Tag pattern (push trigger)    │ Desktop build flags   │
├─────────┼────────────────┼──────────────────────────────┼───────────────────────┤
│ nightly │ Experimental   │ vX.Y.Z-nightly.N             │ embedded-server +     │
│         │ (me + anyone)  │                              │ devtools              │
├─────────┼────────────────┼──────────────────────────────┼───────────────────────┤
│ stable  │ Testers /      │ vX.Y.Z-rc.N                  │ embedded-server only, │
│         │ release        │ vX.Y.Z (plain)               │ no devtools           │
└─────────┴────────────────┴──────────────────────────────┴───────────────────────┘
```

Selection stays **tag-based**. Release branches are not used. Tags are chosen from
three accepted patterns that feed two channels:

- `vX.Y.Z-nightly.N` → nightly channel, prerelease = yes.
- `vX.Y.Z-rc.N`     → stable channel, prerelease = yes.
- `vX.Y.Z`           → stable channel, prerelease = no.

`-rc` lives in the stable channel so testers auto-update along the same
manifest as the eventual release: `0.2.0-rc.1 < 0.2.0-rc.2 < 0.2.0`. It does
*not* get its own channel, manifest, or rolling tag — that would be a third
stream of overhead for one developer.

`nightly` is the experimental stream and gets the devtools feature plus an
adhoc macOS signing opt-out. `stable` (both rc and plain) omits devtools and
requires signed/notarized macOS builds.

If a backport is ever needed (a stable fix while `main` has unfinished work), a
`release/x.y` branch is introduced then and only then — it is a stabilization
seam, not a channel selector.

## Per-channel identity (side-by-side)

Each channel is a **distinct installable app**, so nightly and stable coexist
on the same machine without clobbering each other's data or updater state.

| Channel  | Identifier                 | Product name        | Data root          |
| -------- | -------------------------- | ------------------- | ------------------ |
| stable   | `com.posthaste.mail`       | Posthaste           | (from identifier)  |
| nightly  | `com.posthaste.mail.nightly` | PosthasteNightly | (from identifier)  |

Tauri derives per-platform app-data roots from the bundle identifier, so
distinct identifiers give distinct data roots for free. The checked-in
`tauri.conf.json` holds the stable identity as the default (so local developer
builds are the stable identity); the release workflow overrides identifier,
product name, and updater endpoint at build time via `--config`.

> Note on the "shared runtime, two clients" fallback: that UX requires the
> **separated-runtime topology** (one authority runtime, both clients as
> replicas). In bundled mode each app embeds its own authority + SQLite, so two
> distinct-identifier installs are two independent states, not one shared
> runtime. Distinct identifiers are correct either way; the shared-runtime story
> is a topology concern, not a release-pipeline one.

## Updater manifests and rolling tags

- `nightly` → `latest.json`.
- `stable` → `latest-stable.json`.

The app does **not** follow GitHub's `releases/latest/download/` URL — that URL
flips between channels whenever a different channel is published. Instead, each
channel owns a rolling git tag that the publish job force-updates:

```text
nightly: https://github.com/theoryzhenkov/posthaste/releases/download/nightly/latest.json
stable:  https://github.com/theoryzhenkov/posthaste/releases/download/stable/latest-stable.json
```

`make_latest` is set only for plain stable releases (public discoverability); rc
and nightly releases are marked GitHub prereleases. The rolling tags keep
updater traffic strictly per-channel.

## macOS signing policy

- **Nightly**: Developer ID when secrets are present; ad-hoc when
  `POSTHASTE_MACOS_SIGNING=adhoc` is explicitly requested (CI forks, unsigned
  internal builds).
- **Stable**: fail-closed Developer ID **plus** notarization. The build step
  refuses to proceed on stable if notarization credentials are absent.

## Version scheme (real semver, flipped at v0.2.0)

The app/manifest version is the **real semver** from the tag for the `v0.2.0+`
accepted patterns, so prerelease ordering survives
(`0.2.0-rc.1 < 0.2.0-rc.2 < 0.2.0`):

| Tag                         | App / manifest version |
| --------------------------- | ---------------------- |
| `vA.B.C-nightly.N`          | `A.B.C-nightly.N`      |
| `vA.B.C-rc.N`               | `A.B.C-rc.N`           |
| `vA.B.C` (plain stable)     | `A.B.C`                |

The legacy `v0.1.0-dogfood.N` line keeps the old flattening (`0.1.N`) so
already-shipped dogfood installs keep auto-updating until they move to the
`0.2.0-nightly` stream. New `-dogfood` or `-beta` tags are not accepted.

The flip to real semver happens at the `v0.2.0` cut: a `v0.2.0-*` release is
semver-newer than any `0.1.N`, so no installed client sees a downgrade.

### macOS 3-integer constraint

macOS `CFBundleShortVersionString` wants three non-negative integers, but the
Tauri updater compares the semver `version`. Tauri ties both to the `version`
field, so we keep real semver as `version` and set
`bundle.macOS.bundleVersion` to the prerelease counter (a monotonic build
number) so `CFBundleVersion` is valid. Whether notarization accepts a prerelease
string in `CFBundleShortVersionString` is **empirically gated**: the first
`v0.2.0-rc.*` stable macOS build must pass notarization in CI. If it is
rejected, add a `tauri.macos.conf.json` override that strips `version` to
`A.B.C` for macOS and emit a per-platform manifest version. That override is the
documented fallback; it is not pre-built.

## How the app knows its channel

Compile-time baking (the binary carries it):

- `apps/desktop/build.rs` reads `POSTHASTE_RELEASE_CHANNEL` (default `dev`) and
  re-exports it via `cargo:rustc-env=POSTHASTE_RELEASE_CHANNEL_RESOLVED`, paired
  with `cargo:rerun-if-env-changed`. `lib.rs` reads it with `env!`, so the
  channel is a first-class cargo build input — a stale `target/`/sccache object
  can never silently carry the wrong channel. (The earlier `option_env!` +
  `#[used]` grep-sentinel approach was abandoned: linker `--gc-sections` could
  detach the sentinel's backing constant, making the smoke grep fail opaquely.)
- The binary **self-reports** its channel: run with `--print-release-channel` it
  prints `RELEASE_CHANNEL` and exits before any GUI init. The smoke step runs
  this and compares the output to the expected channel — a direct, toolchain- and
  packaging-independent proof, and a mismatch shows the actual channel.
- Renderer: `VITE_RELEASE_CHANNEL` is set at web-build time and read via
  `import.meta.env.VITE_RELEASE_CHANNEL`.
- A Tauri command (`release_channel`) exposes the Rust constant to the renderer
  so the two cannot silently disagree.

The updater endpoint is also set at build time via `--config`, so both the
channel identity and the endpoint are compile-time-bound to the same channel.

## Workflow shape

1. **`resolve-channel` job** emits `channel`, semver `version`, and `prerelease`
   (derived from the tag; manual dispatch can override the channel).
2. **`build-desktop`** materializes the per-channel policy once via
   `channel-policy.sh` and overrides identity, product name, and updater endpoint
   via `--config`.
3. **Smoke step** unpacks the artifact (AppImage extraction on Linux, the
   `.app.tar.gz` on macOS) and runs the binary's `--print-release-channel`,
   asserting the reported channel equals the expected one.
4. **`generate-updater-manifest.sh`** takes the manifest filename from the
   policy (`latest.json` / `latest-stable.json`).
5. **Publish** uses the `prerelease` flag so rc/nightlies are marked GitHub
   prereleases and only plain stable releases become GitHub's "latest". It then
   force-updates the channel's rolling tag.

## Assertions

| ID                        | Sev.   | Assertion                                                                                  |
| ------------------------- | ------ | ------------------------------------------------------------------------------------------ |
| channel-declared          | MUST   | Every release resolves to a channel: inferred from tag on push, or explicit input on dispatch; unknown tags fail CI. |
| channel-baked             | MUST   | Every desktop binary embeds its channel as a compile-time constant resolved by build.rs from `POSTHASTE_RELEASE_CHANNEL`. |
| channel-self-report-smoke | MUST   | The smoke step runs the binary's `--print-release-channel` and proves the reported channel matches the release channel. |
| channel-policy-single     | MUST   | All per-channel policy is read from one committed policy table by channel, not threaded as booleans. |
| distinct-identity         | MUST   | Nightly and stable use distinct bundle identifiers and product names.                     |
| nightly-devtools           | MUST   | Nightly desktop builds compile with the `devtools` feature.                                |
| stable-no-devtools        | MUST   | Stable desktop builds do not compile with the `devtools` feature.                          |
| channel-rolling-tag       | MUST   | The publish job updates the `nightly` or `stable` rolling tag to the current release.      |
| channel-updater-endpoint  | MUST   | Each built desktop artifact has its channel's rolling-tag updater endpoint baked in at compile time. |
| stable-manifest-name      | MUST   | Stable releases publish `latest-stable.json`; nightly releases publish `latest.json`.    |
| macos-stable-signing      | MUST   | Stable macOS builds require Developer ID signing and notarization credentials.            |
| macos-nightly-signing     | SHOULD | Nightly macOS builds sign with Developer ID when secrets are present or adhoc when opted in. |
| version-real-semver       | MUST   | App/manifest version is the real semver from the tag (prerelease ordering preserved) for the v0.2.0+ line. |
| stable-latest-only        | MUST   | Only plain stable releases are marked GitHub `latest`; rc and nightly releases are marked `prerelease`. |
| nightly-artifact-smoke    | SHOULD | Nightly desktop artifacts pass light smoke before upload.                                  |

## Code anchors

- Release workflow: `.github/workflows/release.yml`
- Channel resolver (tag/input → channel): `tools/release/resolve-channel.sh`
- Channel policy table: `tools/release/channel-policy.sh`
- Version helper: `tools/release/bundle-version-from-tag.sh`
- Updater manifest: `tools/release/generate-updater-manifest.sh`
- Rolling tag: `tools/release/update-rolling-tag.sh`
- Bundle smoke: `tools/release/smoke-desktop-bundle.sh`
- Tauri config: `apps/desktop/tauri.conf.json`
- Channel baking: `apps/desktop/build.rs`, `apps/desktop/src/lib.rs`, `apps/web/src/runtime/releaseChannel.ts`
- Channel self-report: `apps/desktop/src/main.rs` (`--print-release-channel`)
