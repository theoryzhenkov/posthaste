# Runtime domain decomposition — scout brief

`docs/runtime/L2.md` is **869 lines**, far over the 300 limit. Total durable
runtime content is L1 (231) + L2 (869) + L3 (117) + L4 (210) ≈ **1427 lines**.
A flat L0–L3 (4 docs × 300 = 1200 max, and level semantics won't pack them
evenly) **cannot** hold this — runtime *must* become sub-domains. This is the
convention's "too broad → decompose," same as replication's client-link/
backend-link split, but runtime is bigger so it wants 3 concerns, not 2.

## 1. Section map (`##` sections, line ranges, sizes)

| § | Title | Lines | Size | Concern |
|---|---|---|---|---|
| 1 | Implementation boundary | 35–44 | 10 | overview |
| 2 | Runtime components | 45–70 | 26 | internals (assembly) |
| 3 | Bundled application package | 71–97 | 27 | overview/package |
| 4 | Startup and lifecycle | 98–129 | 32 | internals (assembly) |
| 5 | **Runtime handle and adapters** | 130–481 | **352** | **splits — see below** |
| 6 | UI sessions and views | 482–499 | 18 | adapter |
| 7 | View operation flow | 500–550 | 51 | adapter |
| 8 | **Mutation pipeline and catalog** | 551–664 | **114** | mutations |
| 9 | Event and state streams | 665–680 | 16 | internals |
| 10 | Security and storage | 681–769 | 89 | internals (security) |
| 11 | Validation | 770–809 | 40 | distribute → each L3 |
| 12 | Assertions | 810–869 | 60 | distribute → each sub-domain table |

### §5 (the 352-line elephant) cleaves along a clean seam

- **§5.1–5.14 (130–353, ~218 lines) — renderer-facing CONTRACT.** Handle +
  API/renderer adapters, adapter identifiers, session/view/mutation/resource
  *operations*, view descriptors/snapshot/mail-list-data/frames, mutation
  settlement frames, adapter error shape. → **adapter** concern.
- **§5.15 (354–359) — Loopback migration bridge.** Transitional → **eph**.
- **§5.16–5.22 (360–481, ~121 lines) — ASSEMBLY/internals.** Rust contract +
  impl crates, authority-runtime-handle shape, build inputs/outputs,
  adapter-neutral caller context, runtime API groups, adapter boundaries,
  shutdown/task ownership. → **internals** concern.

So §5 is really *contract* (renderer-facing) + *assembly* (runtime guts) glued
together — the single biggest reason L2 is oversized.

## 2. Anchor classes (what constrains any split)

Code `@spec` refs bind to **two** kinds of anchor; both move with their content
and force a **path** repoint (anchor text is preserved):

- **Heading slugs** (number-stripped): `mutation-pipeline-and-catalog` (§8),
  `view-operation-flow` (§7), `view-descriptors` (§5.7),
  `account-status-views` (§7.4), and `runtime/L1#view-operation` (L1 §5).
- **Assertion-row IDs** (in the §12 table, lines 819–849): `runtime-builder-
  transport-free`, `runtime-handle-transport-neutral`, `runtime-shutdown-
  handle`, `renderer-one-frame-stream`, `runtime-owned-roots`,
  `provider-secrets-runtime-store`. These move into the **sub-domain's own
  assertions table**, so each assertion must be filed with its concern.

## 3. Decomposition options

All three keep `runtime/L1` as the shared overview (it is already a clean
231-line contract: roles, authority runtime, UI runtime contract, view
operation, deployment transparency) and move `runtime/L4` "code patterns" into
the relevant sub-domain `L3`s, with §5.15 loopback → eph.

### Option A — 2 sub-domains: `runtime/{surface, authority}`
- `runtime/surface/{L1,L2,L3}` = §5.1–5.14 + §6 + §7 + §8's contract face.
- `runtime/authority/{L1,L2,L3}` = §2 + §4 + §5.16–5.22 + §8 pipeline + §9 + §10.
- **Lands <300?** Yes, but `authority` is a grab-bag (~400 lines of assembly +
  mutation execution + security + streams) and the 16-ref
  `mutation-pipeline-and-catalog` anchor splits awkwardly between surface
  (contract) and authority (execution).
- Mirrors replication's 2-way split. Simplest count; weakest cohesion.

### Option B — 3 sub-domains: `runtime/{adapter, mutations, security}` + slim top-level
- top-level `runtime/L2`/`L3` keep assembly (§2,§4,§5.16–5.22), streams (§9).
- `runtime/adapter/{L1,L2}` = §5.1–5.14 + §6 + §7.
- `runtime/mutations/{L1,L2}` = §8 (one clean home for the 16-ref anchor).
- `runtime/security/{L1}` = §10.
- **Lands <300?** Yes. Cleanest anchor homing. Cost: 3 sub-dirs **plus** a
  still-substantial top-level runtime/L2 (assembly), i.e. assembly has no
  sub-domain identity though it is a real concern — slightly asymmetric.

### Option C (RECOMMENDED) — 3 sub-domains by anchor cluster: `runtime/{adapter, mutations, internals}`
No content stays at an oversized top level; every concern is a sub-domain.
- `runtime/L1` — shared overview (keep ~as-is; holds `#view-operation`).
- `runtime/adapter/{L1,L2,L3}` — renderer-facing surface: §5.1–5.14 (L1
  contract ~200), §6 + §7 (L2 flow/registries ~130), patterns (L3 ~60).
  Homes: `view-operation-flow`, `view-descriptors`, `account-status-views`,
  `renderer-one-frame-stream`.
- `runtime/mutations/{L1,L2}` — §8 catalog+contract (L1 ~70) + pipeline/
  settlement (L2 ~60). Homes the **16-ref** `mutation-pipeline-and-catalog`.
- `runtime/internals/{L1,L2,L3}` — assembly/build/handle/crates (§2, §5.16–
  5.22), startup/lifecycle (§4), streams (§9), **security & storage (§10)**,
  absorbing L4 build/test patterns. ~180 per level. Homes: `runtime-builder-
  transport-free`, `runtime-handle-transport-neutral`, `runtime-shutdown-
  handle`, `runtime-owned-roots`, `provider-secrets-runtime-store`,
  `runtime-health`, L4 `authority-build-order`.
- §1 boundary + §3 package → `runtime/L1`. §11 validation → each sub-domain
  L3. §12 assertions → split into each sub-domain's table.

**Why C:** (a) cleaves §5 exactly on its contract|assembly seam; (b) the
heavily-referenced mutation anchor (16) gets one unambiguous home; (c) no
oversized leftover at the top level (B's weakness); (d) "internals" absorbs
security so we avoid an over-fragmented tiny `security/` sub-domain — but if
security is expected to evolve independently, promote §10 to `runtime/security/`
as a 4th sub-domain (the one knob to decide). 3 sub-domains vs replication's 2
is justified by runtime being ~2× the content.

## 4. Full code-ref repoint list (every anchor a split must preserve/repoint)

40 refs total. Path repoints under **Option C** (anchor text unchanged):

**Heading-slug anchors**
| Anchor (refs) | Now | → Option C |
|---|---|---|
| `mutation-pipeline-and-catalog` (16) | runtime/L2 | runtime/mutations/L1 |
| `view-operation-flow` (4) | runtime/L2 | runtime/adapter/L2 |
| `account-status-views` (2) | runtime/L2 | runtime/adapter/L2 |
| `view-descriptors` (1) | runtime/L2 | runtime/adapter/L1 |
| `view-operation` (2) | runtime/L1 | runtime/L1 (unchanged) |

**Assertion-row IDs** (move row into sub-domain assertions table)
| Anchor (refs) | Now | → Option C |
|---|---|---|
| `runtime-builder-transport-free` (3) | runtime/L2 | runtime/internals/L2 |
| `runtime-handle-transport-neutral` (2) | runtime/L2 | runtime/internals/L1 |
| `runtime-shutdown-handle` (2) | runtime/L2 | runtime/internals/L2 |
| `renderer-one-frame-stream` (2) | runtime/L2 | runtime/adapter/L1 |
| `runtime-owned-roots` (1) | runtime/L2 | runtime/internals/L1 (security) |
| `provider-secrets-runtime-store` (1) | runtime/L2 | runtime/internals/L1 (security) |
| `runtime-health` (1) | runtime/L2 | confirm at placement (adapter §7.4 / internals) |

**L4 anchors** (L4 "code patterns" folds into sub-domain L3)
| Anchor (refs) | Now | → Option C |
|---|---|---|
| `authority-build-order` (4) | runtime/L4 | runtime/internals/L3 |
| `account-resource-linkage-runtime-owned` (1) | runtime/L4 | runtime/internals/L3 (confirm) |
| `account-mutation-contract-pattern` (1) | runtime/L4 | runtime/mutations/L2 or internals/L3 |

**L3 anchor**
| Anchor (refs) | Now | → Option C |
|---|---|---|
| `event-subscription-runtime-backed` (1) | runtime/L3 | runtime/internals/L3 (current L3 lands here) |

**Files touched (repoint):** `crates/posthaste-server/src/app_state.rs`,
`crates/posthaste-authority-runtime/{tests/authority_runtime_handle.rs,
src/{build.rs,sessions.rs,views.rs,backend.rs,secret.rs,supervisor/tests.rs}}`,
`apps/web/src/{components/message-list/useRuntimeMailListView.ts,
runtime/useRuntimeObjectView.ts,hooks/{useRuntimeUndoRedo,useDaemonEvents,
useAccountsView,useEmailActions}.ts}`. Mostly path-only; bulk-sed safe because
anchor text is preserved.

## 5. One decision for the user
Pick the structure (recommend **C**), and decide the single knob: **is §10
security its own `runtime/security/` sub-domain, or folded into
`runtime/internals/`?** Everything else is mechanical placement under the
convention. No edits were made.
