---
scope: L1
summary: "Convention + migration plan: SPECial docs return to L0–L3 depth-only; architectural initiatives move to eph/; per-section state markers track intent-vs-reality with pointers to eph plan/state docs."
modified: 2026-06-24
reviewed: 2026-06-24
lifecycle: ephemeral
type: PLAN
depends:
  - path: docs/index
dependents: []
---

# SPECial state-tracking convention & restructure plan

This is the **contract** for the docs restructure. Every agent (including
parallel-workspace agents) applies this convention identically. When the
restructure is complete, this convention is promoted into the project's SPECial
reference and this PLAN doc is deleted.

## 1. The level axis is depth, never time

SPECial levels describe the **current system** at increasing depth:

- **L0** — why the domain exists, stakes.
- **L1** — interfaces, invariants, rules (work *with* the domain).
- **L2** — components and how they interact (modify/extend the domain).
- **L3** — implementation patterns, edge cases, performance (debug/optimize).

There is **no L4+**. A higher number is a closer look at the same standing
thing, not a later initiative. Architectural initiatives, migrations, and
forward designs are **ephemeral** and live in `docs/eph/` as `DESIGN`/`PLAN`/
`RFC` docs with `lifecycle: ephemeral`. When an initiative lands, its realized
shape is folded into the relevant L0–L3 section and the eph doc is deleted.

## 2. Size: every doc < 300 lines

Achieve this by **pushing deeper detail down a level** or **splitting a
too-broad domain into sub-domains** (new directories with their own L0–L3).
Never satisfy the limit by cutting a doc mid-topic — that breaks level
semantics. A doc that is too broad even after pushing detail down is a signal
the *domain* needs decomposition; flag it rather than guessing the cut.

## 3. Per-section state markers (intent vs reality)

SPECial docs state **intent**. The gap between intent and shipped code is
tracked per top-level `##` section.

- **Default in frontmatter:** every L0–L3 doc carries `state: realized` meaning
  its code matches it. A doc with no per-section markers is fully realized.
- **Deviations only:** a `##` section whose code does *not* match the spec
  carries one marker line directly under the header. Clean section = realized.
- **Granularity:** markers go on top-level `##` sections **only**, never `###`.

### Marker syntax

Reuses the project's existing `[::…]` directive idiom (cf. `[::TODO]`):

```markdown
## 5. Coverage subscriptions

[::state planned plan=eph/PLAN-L2-client-link-unification#coverage]
```

### State vocabulary

| State | Meaning | Required pointer |
| --- | --- | --- |
| `realized` | code matches this section (frontmatter default; no marker needed) | none |
| `partial` | partly built; remainder in flight | `plan=` → eph plan/design `path#anchor` |
| `planned` | intent only, not built | `plan=` → eph plan/design `path#anchor` |
| `diverged` | code intentionally differs from this spec right now | `state=` → eph report `path#anchor` |

`plan=` points to the forward-work eph doc (`PLAN`/`DESIGN`). `state=` points to
the eph doc describing actual current behavior (`REPORT`). Pointers are
`path#anchor`, root-relative, no `.md` extension.

### Lifecycle

When the work behind a `planned`/`partial`/`diverged` marker lands, fold the
result into the section and **remove the marker** (back to realized default).
Markers exist only while a gap exists, so they cannot rot into lies.

### Domain-state ledger (the payoff)

```
grep -rFn '[::state' docs
```

(`-F` fixed-string avoids regex/shell quoting pitfalls with the `[`.) Yields the entire map of where the project's reality and intent diverge, by
section, with pointers to the tracking work. This is the machine-readable
domain-state index agents consult before planning.

## 4. Staleness discipline

Docs are currently non-stale. Any doc touched here gets `modified` and
`reviewed` bumped to the edit date, and its `depends`/`dependents` graph
repointed so no dangling edges remain. Keep the graph consistent.

## 5. Migration map

### 5.1 Fold-backs (retire L4+)

- **replication/L4,L5,L6** → move to `eph/` as `DESIGN`/`PLAN`. Fold realized
  parts into `replication/L2`/`L3`; mark not-yet-built parts `planned`/`partial`
  pointing at the eph docs. (Pattern-setter slice.)
- **backend/L4**, **runtime/L4** ("code patterns") → merge durable patterns into
  each domain's `L3`; migration framing → eph. Fix `backend/L4`'s duplicate
  `## 13. Assertions` heading.
- **client/L4** → becomes `client/L3` (client has no L3 today).

### 5.2 Over-300 splits

- `backend/L1` (310), `state/mail/L2` (319) — trim or push one section down.
- `backend/L2` (583) — push §7 Runtime flows + §10 Extension points to L3.
- `api/L1` (497), `api/L2` (375) — push `api/L1 §3` endpoint inventory toward a
  generated artifact / L2 (decision pending).
- `runtime/L2` (869) — **needs sub-domain decomposition**, separate design step;
  do not split mechanically.

## 6. Realized-vs-reality is verified, not guessed

When folding an initiative, **verify against code and the jj log** which slices
landed. Do not infer status from prose tense. Cite the symbol or commit that
proves a section is `realized`.
