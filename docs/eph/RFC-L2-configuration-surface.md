---
scope: L2
summary: "RFC — recoverability-first: clean recovery from corrupted/undefined states (incl. why 'Reset database' is a no-op) and a robust, file-as-source-of-truth configuration foundation that a human or an LLM can edit without a GUI harness. TOML is the single source of truth; the IndexedDB client replica is kept as a rebuildable cache. Companion matrix carries the per-parameter table + TOML schema."
modified: 2026-06-28
reviewed: 2026-06-28
state: planned
depends:
  - path: docs/L1-accounts
    section: "TOML schema"
    local: "3. Configuration model — TOML files are the source of truth"
  - path: docs/eph/RFC-L2-configuration-matrix
    local: "3. Configuration model — TOML files are the source of truth"
---

# RFC — Configuration & Repair Surface (+ Settings UX)

Status: **proposal** (`state: planned`). Authored by a coordinator-led team
(repair / taxonomy / UX facets); **revised 2026-06-28 per owner review** —
recoverability-first; TOML files are the single source of truth (LLM/CLI-editable,
no harness); the IndexedDB replica stays a rebuildable cache; no telemetry. The
full per-parameter table is in
[RFC-L2-configuration-matrix](RFC-L2-configuration-matrix.md).

## 1. Problem & priority [::state planned]

The spine, per the owner: the app must **(a) recover cleanly from corrupted /
undefined states**, and **(b) expose its necessary basics as file-configurable**
— editable by a human *or an LLM without special harnesses*. The full settings
redesign and advanced/per-account knobs are explicitly secondary.

Three concrete gaps:
1. **Repair doesn't work.** "Reset database & restart" cannot fix the symptom
   users actually hit (§2).
2. **Config is built but hidden** and not robustly file-editable — `app.toml`
   already carries cache caps, logging, daemon tuning, link topology, automations
   (the user's "limit cache size" is *partly already implemented*), but it is
   invisible and the file/app coupling is fragile (§3, §6).
3. **Settings UI is a junk drawer** — addressed, but deferred (§5).

## 2. The "Reset database" no-op — root cause [::state planned]

The marker mechanism *works*: `desktopRepair.ts` →
`invoke('request_database_repair')` writes `.repair-requested` into the state
root; on relaunch `posthaste-store::open_with_repair` quarantines + rebuilds
`mail.sqlite` and consumes the marker (test-covered).

**But it is scoped to the wrong store.** The mail-list views are computed from
the **client IndexedDB replica `posthaste-replica`** (outbox + undo-history),
which lives in the webview's storage **outside the app data dir**. That replica
— not `mail.sqlite` — is the documented cause of "views stuck loading forever"
(the durable-replica schema-drift / unguarded-rehydration class; see
`apps/web/src/runtime/replica/replicaDatabase.ts`). **Nothing anywhere clears
it** (no `indexedDB.deleteDatabase` in the tree); it survives the rebuild, the
relaunch, even deleting the app dir. So the only repair button rebuilds the
wrong store → "does nothing." Secondary: the embedded build calls
`DatabaseStore::open` and **discards the `RepairReport`** → zero feedback, no
re-sync kick.

## 3. Configuration model — TOML files are the source of truth [::state planned]

**One source of truth: on-disk TOML.** Everything meaningful — accounts,
transport, rules, caps, topology, *and* UI prefs (theme / keymap / notification
policy) — lives in the TOML config files (`app.toml`, `sources/*`,
`smart-mailboxes/*`). The decision: a human or an LLM configures the app by
**editing files or running CLI verbs — no GUI harness**. Only **ephemeral
per-device view-state** (window / panel / column geometry, dev-tools toggle)
stays in the renderer's `localStorage`; it is not configuration and no one
hand-edits it.

> Today theme/density live in a `localStorage` "client-preferences" blob —
> opaque, not file-editable. This RFC **moves them into TOML** (one-time import).
> Cross-device sync is deferred and will be solved by syncing the *files*, not a
> browser blob.

**Making the file coupling robust (the point of fragility, and its fix).**
Files-as-truth only works if the app and a hand/LLM editor never clobber each
other. Required mechanism (the heart of P1):

- **Single writer-of-record** — one config service owns reads + writes; the UI
  writes *through* it; the file is canonical.
- **Lossless, format-preserving round-trip** — writes via `toml_edit`, so
  comments, ordering, and *unknown* sections survive a UI save. This fixes the
  `AppToml::from_app_settings` footgun (§6.6) that silently drops any section not
  in its preserve list.
- **Reload on external change** — the app watches the config files and re-reads
  on an external edit, so an LLM/script edit is picked up live. (This is itself a
  recovery path: a wedged config is fixable by editing the file.)
- **Schema + validation** — a bad edit is reported, never silently dropped.

**Exposure tiers** (the UI is a curated editor over the TOML): **UI-basic** /
**UI-advanced** (one "Advanced ▸" disclosure per pane) / **TOML-only** (reached
via an "Open config file →" escape hatch) / **hidden**. The full param→tier
mapping is the [matrix](RFC-L2-configuration-matrix.md).

## 4. Repair & recovery — the priority surface [::state planned]

Repair *actions* are imperative verbs (CLI + UI buttons), **not** TOML keys.
The ladder, least → most destructive:

| knob | clears | blast radius | surface |
|---|---|---|---|
| **Reset local views / replica** *(NEW — fixes §2)* | `posthaste-replica` IndexedDB | unsent/queued mutations + undo history lost; **no mail lost** (re-hydrates) | Troubleshooting + corruption notification |
| **Rebuild local DB** *(exists)* | `mail.sqlite` (quarantined) | full local re-sync; accounts/secrets kept | Troubleshooting + notification |
| **Clear caches** *(NEW; policy half-built)* | cached bodies/raw/attachments | re-fetch on demand; safe | Troubleshooting; auto via caps |
| **Full re-sync account** *(exists)* | per-account cursors | per-account re-download | Account editor |
| **Forget & re-add account** *(exists)* | account config + its mail | re-auth to restore | Account editor |
| **Factory reset** *(NEW)* | everything local: SQLite/config dir **and** webview store | all local state + queued ops gone; mail re-syncs | Troubleshooting (type-to-confirm) + CLI |
| **Edit / reload config file** *(NEW — recovery via §3 reload)* | nothing; re-reads canonical TOML | fixes a wedged/invalid config without the GUI | CLI / file watch |

**The user-facing one-click becomes "Repair & restart" = reset replica → write
DB-rebuild marker → relaunch** (replica first — cheap, fixes the common case).
**Scriptability:** CLI verbs over the marker protocol —
`posthaste repair --database|--replica|--caches|--factory`. **Fix the feedback
gap:** switch the embedded build to `open_with_repair`, surface the
`RepairReport` (notification + auto re-sync).

### Storage-engine decision — keep IndexedDB; the replica is a rebuildable cache [::state planned]

Recorded so it is not re-litigated. The client replica runs **in the renderer**;
IndexedDB is renderer-local and the **only durable client store that works in a
plain browser** — and the project targets a hosted/web runtime. Native SQLite
would add an IPC bridge on the reactive-store hot path *and* break the web
deployment (or force a dual backend). The replica **re-derives from the server**;
only never-sent outbox ops are irreplaceable (surfaced on reset as "N unsent
changes will be discarded"). Therefore: **do not migrate** — make reset+rebuild
clean (P0), **co-locate the webview data dir under the app data dir at launch**
(Tauri sets it at init → inspectable + wipe-able as a folder), and harden
versioning. A future SQLite-semantics option is `wa-sqlite` on OPFS (still
browser storage, no native bridge), never native SQLite.

## 5. Settings UX — target, deferred to P2 [::state planned]

The revamp stands as the target but is **not the priority**. Six clear
sections (`General · Appearance · Accounts · Mailboxes & Rules · Storage & Sync ·
Troubleshooting`), each basic-first with one "Advanced ▸" disclosure and a
consistent "Open config file →" escape hatch; dissolve the "General" junk drawer
into **Troubleshooting**; add the **smart-mailbox Role selector** (the model
already carries `role`); standardize confirmations on `AlertDialog`. P1 surfaces
only the basics needed for recovery + core config; the full IA follows in P2.

## 6. Key decisions (resolved) [::state planned]

1. **TOML files = single source of truth** for all meaningful config including
   UI prefs; `localStorage` holds only ephemeral geometry. LLM/CLI-editable, no
   harness. Cross-device later, via file sync.
2. **Robust file coupling** (single-writer service + `toml_edit` lossless
   round-trip + reload-on-change + validation) — what makes files-as-truth
   non-fragile; the core of P1.
3. **Repair = imperative verbs** (CLI + UI buttons); composite **"Repair &
   restart"** = reset replica → DB rebuild → relaunch.
4. **Keep IndexedDB; replica = rebuildable cache** (co-locate + reset, do not
   migrate to SQLite) — §4.
5. **State location:** `storage.data_dir` (config + sqlite) is a TOML knob; the
   webview/IndexedDB dir is set at **launch** (co-located under the app dir), not
   a hot TOML knob.
6. **Round-trip footgun — must-fix:** `AppToml::from_app_settings` rebuilds
   field-by-field and preserves only daemon/logging/cache/link → any new section
   is dropped on the next save. The P1 service must preserve unknown sections.
7. **No telemetry** (dropped entirely).
8. **Runtime update channel** kept (build-time today; needs updater support).

## 7. Phased plan — recoverability-first [::state planned]

- **P0 — Repair / recovery (the priority).** Replica reset (`deleteDatabase` +
  re-init); re-scope "Repair & restart" to the composite; surface `RepairReport`
  + auto re-sync; factory reset (both stores); co-locate the webview data dir;
  CLI `posthaste repair --*`.
- **P1 — Robust config foundation.** The single-writer config service +
  `toml_edit` lossless round-trip + reload-on-change + validation + the
  preserve-unknown fix; migrate `localStorage` prefs → TOML; surface the basics
  + the "Open config file" affordance.
- **P2 — Settings IA revamp.** Six-section tree, progressive disclosure,
  smart-mailbox Role selector, `AlertDialog`/copy standardization.
- **P3 — Extended config.** Per-account sync/cache overrides, retention,
  signature, runtime update-channel switch, notification detail.

## 8. Risks & open questions [::state planned]

- Webview data-dir relocation feasibility per platform (WebKitGTK / WKWebView /
  WebView2) — verify launch-time co-location works on each.
- `toml_edit` round-trip across all sections; one-time import of existing
  `localStorage` prefs → TOML.
- Per-account override precedence vs global (P3) — define merge/clamp (mirror the
  existing `hard ≥ soft` clamp).
- `daemon.runtime.*` stays TOML-only / internal, never UI.
