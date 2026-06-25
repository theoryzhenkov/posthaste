---
scope: L2
summary: "Release channels as a first-class product concept: channel is declared (tag-inferred for push, explicit for manual dispatch), baked into the binary, and drives a single policy table covering distinct per-channel identity, updater manifest, devtools, macOS signing, and smoke gates."
modified: 2026-06-25
reviewed: 2026-06-25
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
│ nightly │ Dogfood / dev  │ vX.Y.Z-dogfood.N             │ embedded-server +     │
│         │                │ vX.Y.Z-nightly.*             │ devtools              │
├─────────┼────────────────┼──────────────────────────────┼───────────────────────┤
│ stable  │ Public beta /  │ vX.Y.Z-beta.N                │ embedded-server only, │
│         │ release        │ vX.Y.Z-rc.N                  │ no devtools           │
│         │                │ vX.Y.Z (plain)               │                       │
└─────────┴────────────────┴──────────────────────────────┴───────────────────────┘
```

Selection stays **tag-based**. Release branches are not used: `stable` is a
blessed commit promoted from `main`, not a stabilized branch. If a backport is
ever needed (a stable fix while `main` has unfinished work), a `release/x.y`
branch is introduced then and only then — it is a stabilization seam, not a
channel selector.

## Per-channel identity (side-by-side)

Each channel is a **distinct installable app**, so nightly and stable coexist
on the same machine without clobbering each other's data or updater state.

| Channel  | Identifier                 | Product name        | Data root          |
| -------- | -------------------------- | ------------------- | ------------------ |
| stable   | `com.posthaste.mail`       | Posthaste           | (from identifier)  |
| nightly  | `com.posthaste.mail.nightly` | Posthaste Nightly | (from identifier)  |

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

`make_latest` is set only for stable releases (public discoverability), while
the rolling tags keep updater traffic strictly per-channel.

## macOS signing policy

- **Nightly**: Developer ID when secrets are present; ad-hoc when
  `POSTHASTE_MACOS_SIGNING=adhoc` is explicitly requested (CI forks, unsigned
  internal builds).
- **Stable**: fail-closed Developer ID **plus** notarization. The build step
  refuses to proceed on stable if notarization credentials are absent.

## Version scheme (real semver, flipped at v0.2.0)

The app/manifest version is the **real semver** from the tag, preserving
prerelease ordering so `0.2.0-beta.5 < 0.2.0-rc.1 < 0.2.0`:

| Tag                         | App / manifest version |
| --------------------------- | ---------------------- |
| `vA.B.C-dogfood.N`          | `A.B.C-dogfood.N`      |
| `vA.B.C-nightly.N`          | `A.B.C-nightly.N`      |
| `vA.B.C-beta.N`             | `A.B.C-beta.N`         |
| `vA.B.C-rc.N`               | `A.B.C-rc.N`           |
| `vA.B.C` (plain stable)     | `A.B.C`                |

This replaces the old flattening (`vA.B.C-dogfood.N → A.B.N`), which destroyed
prerelease ordering. Flattening is retained **only** for the legacy `0.1.0-dogfood.N`
line so already-shipped dogfood installs (version `0.1.N`) keep updating. The
flip happens at the `v0.2.0` cut: the next release must be `v0.2.0-*`, which is
semver-newer than any `0.1.N`, so no installed client sees a downgrade.

### macOS 3-integer constraint

macOS `CFBundleShortVersionString` wants three non-negative integers, but the
Tauri updater compares the semver `version`. Tauri ties both to the `version`
field, so we keep real semver as `version` and set
`bundle.macOS.bundleVersion` to the prerelease counter (a monotonic build
number) so `CFBundleVersion` is valid. Whether notarization accepts a prerelease
string in `CFBundleShortVersionString` is **empirically gated**: the first
`v0.2.0-beta.*` stable macOS build must pass notarization in CI. If it is
rejected, add a `tauri.macos.conf.json` override that strips `version` to
`A.B.C` for macOS and emit a per-platform manifest version. That override is the
documented fallback; it is not pre-built.

## How the app knows its channel

Compile-time baking (the binary carries it):

- Rust: `const RELEASE_CHANNEL: &str = option_env!("POSTHASTE_RELEASE_CHANNEL").unwrap_or("dev");`
- A `#[used]` sentinel string `posthaste-release-channel=<channel>` is embedded so
  the smoke step can prove which channel a binary was built on.
- Renderer: `VITE_RELEASE_CHANNEL` is set at web-build time and read via
  `import.meta.env.VITE_RELEASE_CHANNEL`.
- A Tauri command exposes the Rust constant to the renderer so the two cannot
  silently disagree.

The updater endpoint is also set at build time via `--config`, so both the
channel identity and the endpoint are compile-time-bound to the same channel.

## Workflow shape

1. **`resolve-channel` job** emits a single output, `channel` (and the derived
   semver `version`). For `workflow_dispatch` it reads an explicit `channel`
   input; for tag-push it infers from the tag.
2. **`build-desktop`** materializes the full policy for that channel into
   `GITHUB_ENV` by calling `channel-policy.sh <channel>` once, then:
   - passes `--features devtools` only when the policy says so;
   - overrides `tauri.conf.json` identifier / productName / updater endpoint via
     `--config`;
   - sets `POSTHASTE_RELEASE_CHANNEL` and `VITE_RELEASE_CHANNEL` at build;
   - enforces stable macOS signing + notarization.
3. **Smoke step** extracts the AppImage, runs `--version`/`--help`, and greps
   the binary for the `posthaste-release-channel=<channel>` sentinel — a real
   assertion that the binary was built on the expected channel.
4. **`generate-updater-manifest.sh`** takes the manifest filename from the
   policy (`latest.json` / `latest-stable.json`).
5. **Publish** force-updates the `nightly`/`stable` rolling tag and sets
   `make_latest` only for stable.

## Assertions

| ID                        | Sev.   | Assertion                                                                                  |
| ------------------------- | ------ | ------------------------------------------------------------------------------------------ |
| channel-declared          | MUST   | Every release resolves to a channel: inferred from tag on push, or explicit input on dispatch; unknown tags fail CI. |
| channel-baked             | MUST   | Every desktop binary embeds its channel as a compile-time constant and sentinel.          |
| channel-sentinel-smoke    | MUST   | The smoke step proves the binary's baked channel matches the release channel.             |
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
| stable-artifact-smoke     | MUST   | Stable desktop artifacts pass full smoke (extract, version/help, channel sentinel).        |
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
- Channel baking: `apps/desktop/src/lib.rs`, `apps/web/src/runtime/releaseChannel.ts`
