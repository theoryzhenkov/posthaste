---
scope: L2
summary: "RFC (draft) — draft identity, consolidated: one durable draft_registry as the single authority for the stable→live-draft mapping (alias/projection dual-source eliminated), one DraftRegistry port every draft operation resolves through (resolve-at-flush; stable-key-carrying ops), provider rotation owned by the adapters behind MailGateway, deterministic identity birth (subsumes the in-flight DS2 create-id), and the deletion of the special-case lattice (the resolve_draft_entity if-fallback, the D133 idempotent_redelivery threading, the attempts>0 heuristic, the scattered liveness probes). Decisions D135–D141 (reserved D135–D144); migration M68–M73. Supersedes/completes RFC-L2-drafts Part 2 (D131's 'ONE place' promise, finally made true)."
modified: 2026-07-05
reviewed: 2026-07-05
lifecycle: ephemeral
type: RFC
state: draft
depends:
  - path: eph/RFC-L2-drafts
  - path: eph/BETA-READINESS
  - path: eph/RFC-L2-provider-reliability
dependents: []
---

# RFC — Draft identity, consolidated (draft)

Status: **draft — for ratification review.** This is a **REFACTOR of one
subsystem's identity model**, not a rewrite: the ports/adapters/outbox/replica
structure is sound and is reused as-is. What it consolidates is the seven-patch
accretion around *which id names a draft and who answers that question*
(M60 send-consume → M61 discard → M62 idempotent-save/twin → M63 stable-id
surfacing → M64 optimistic discard → M65 save routing + DS2/DS3 point fixes →
the in-flight DS2 deterministic-create-id). Each patch was correct locally;
none owned the identity model, so each added a resolution site, a guard, or a
flag instead of removing one.

**Reserved ranges:** decisions **D135–D144** (this RFC uses D135–D141);
migration steps **M68–M79** (this RFC uses M68–M73). D125–D134 / M60–M67
belong to RFC-L2-drafts and are preserved, not renumbered.

---

## 1. The identity reality today (surveyed 2026-07-05, all file:line current)

### 1.1 The stable identity and where it lives

A draft's durable handle is the client-minted key `draft-local-<uuid>`
(minted in `save_draft` when no key is supplied —
`crates/posthaste-domain-service/src/service/outbox.rs:361-363`), carried as
the `X-Posthaste-Draft-Id` header (`DRAFT_ID_HEADER`,
`crates/posthaste-domain-model/src/model/mod.rs:114`). It is stamped by both
adapters at save time:

- JMAP: header on the `Email/set` create —
  `crates/posthaste-engine/src/live_compose/draft.rs:47-57`.
- IMAP: header line prepended to the RFC822 message before APPEND —
  `crates/posthaste-imap/src/gateway/draft.rs:73-83`.

Sync reads it back and projects it as the `message.draft_id` column
(schema: `crates/posthaste-store/src/db/schema/sql.rs:40`; JMAP fetch property:
`crates/posthaste-engine/src/sync/email.rs:424-428`; IMAP header parse:
`crates/posthaste-imap/src/message.rs:87-92`), surfaced on details
(`crates/posthaste-store/src/query/details.rs:15,47,117,146`) and — since M63 —
on list rows (`crates/posthaste-store/src/query/summaries.rs:53-75,162-184`).

The handle is *necessary* because the underlying provider id is unstable on
**both** providers:

- JMAP drafts are immutable — every save is create-new + destroy-old in one
  `Email/set` (`crates/posthaste-engine/src/live_compose/draft.rs:14-15,97-98`),
  so the Email id rotates E1→E2→… per autosave.
- IMAP saves are APPEND-then-delete — the UID (and hence the UID-encoded
  canonical id) changes per save
  (`crates/posthaste-imap/src/gateway/draft.rs:42-44,107-109`).

### 1.2 TWO sources of truth for one fact

The fact "stable key K currently names live entity E" is answerable from two
independent stores, each maintained by a different writer:

| Source | Written by | Fresh in | Stale in |
|---|---|---|---|
| `draft_alias` table (`sql.rs:156-161`) | this runtime only: `set_draft_alias` at save-enqueue (`domain-service outbox.rs:372`), `update_draft_alias_entity` at flush reconcile (`outbox.rs:637-641`), `remove_draft_alias` at delete-**enqueue** (`outbox.rs:428`) | the in-session window: enqueue→flush→pre-sync, including mid-rotation | rotations observed only via sync (another device edited the draft; a server-side change) — **sync never touches the alias**, so the alias can point at a destroyed id forever |
| `message.draft_id` projection (`sql.rs:40`) | sync only, from the round-tripped header | cross-restart, cross-device, anything the server has confirmed | the in-session window: an offline/unflushed save has **no projection row at all**; a just-flushed rotation isn't projected until the next sync |

`resolve_draft_entity` (`crates/posthaste-store/src/outbox.rs:321-356`) welds
them together with a precedence `if`: alias wins; on miss, `SELECT id FROM
message WHERE draft_id = key`. The comment at `outbox.rs:330-338` documents
the precedence (D131) — it is honest, and it is exactly the smell: **each
source is authoritative in a regime the other cannot see**, so no precedence
order is safe. Alias-first is wrong after an other-device rotation (alias
stale → resolves to a destroyed id → provider `notFound` → the whole D133
apparatus exists to absorb that). Projection-first would be wrong mid-session
(no row yet, or a pre-rotation row).

### 1.3 The scattered resolution sites and their per-site special cases

Every draft operation re-derives liveness/identity itself, each with its own
patch-era guard:

1. **save** (`domain-service outbox.rs:351-396`): resolve → alias-hit =
   `DraftUpdate`; miss = self-alias `set_draft_alias(key, key)` **plus** the
   `draft_message_exists` probe (`outbox.rs:378-382,511-521` — mailbox
   membership as a liveness proxy) to distinguish "resumed by rotating
   provider id / legacy draft" from "brand new". A third way of asking "is
   this draft live?".
2. **delete (send-consume path)** (`outbox.rs:408-430`): resolve →
   `.unwrap_or_else(|| key.clone())` identity-fallback (`outbox.rs:415-418`),
   then `remove_draft_alias` **at enqueue** (`outbox.rs:428`) — the identity
   is forgotten before the provider confirms the destroy. If the destroy
   later fails permanently, the mapping is gone while the draft lives; a
   subsequent save re-bootstraps via the `draft_message_exists` probe. It
   works, by accident of the probe — the design does not guarantee it.
3. **discard (user path, M64)** (`outbox.rs:445-504`): resolves *again*, then
   performs a **third** liveness read — `get_message_mailboxes` +
   `get_message_summary` (`outbox.rs:459-472`) — to decide whether to surface
   `NotFound` (D133) before delegating to (2).
4. **send-consume trigger** (`consume_draft_after_send`,
   `outbox.rs:778-809`): resolves a **fourth** time, with its own
   already-consumed guard `resolve(...).is_some() || draft_message_exists(...)`
   (`outbox.rs:802-806`).
5. **flush reconcile** (`outbox.rs:627-643`): a rotation returned by the
   gateway is written **twice**, through two parallel rewrite APIs —
   `reconcile_operation_entity_id` (outbox rows) *and*
   `update_draft_alias_entity` (alias) — mirrored state that must be kept in
   step by every future author.
6. **the D133 flag lattice**: `idempotent_redelivery` is threaded as a bool
   through the op payload (`outbox.rs:426`), re-parsed at push
   (`outbox.rs:871-875`), passed through the `MailGateway` port
   (`crates/posthaste-domain-service/src/ports/gateway.rs:190-196,212-216`),
   and consumed as a `notFound ⇒ Ok` mask in the JMAP adapter
   (`live_compose/draft.rs:122-133,160-177`). The update path derives it from
   `attempts > 0` (`outbox.rs:852-857`) — a proxy that is **wrong for a
   transient failure before the request ever reached the provider** (attempts
   bumps at `outbox.rs:694-698` on `Transient` too), silently widening the
   DS3 mask on exactly the retry it was built to protect.
7. **the DS2 hole** (BETA-READINESS.md:37, in-flight): the JMAP draft create
   is anonymous (`live_compose/draft.rs:38` — contrast
   `send.rs:97` `create_with_id(phsend-…)`), and the `Transient` flush arm
   re-queues without any entity reconciliation (`outbox.rs:690-705`) — a
   lost-response retry re-creates and mints a durable twin. The in-flight fix
   (deterministic create-id) is being bolted onto the save path rather than
   modeled as the identity's birth.

### 1.4 Why each patch added debt instead of consolidating

Each landed under an active user-facing bug with a tight blast-radius mandate:
M60's `notFound⇒Ok` mask (regression documented at RFC-L2-drafts §"Field bug")
was narrowed by D133 rather than removed, because removal required fixing
*resolution freshness* first; M63 surfaced the stable id on rows but left both
mapping stores in place because touching sync's writer set was out of scope;
DS3 added the `destroyed(replace)` inspection with the `attempts>0` heuristic
because the op model had no honest "this is a redelivery" fact to consult; the
DS2 fix reaches for a deterministic create-id because nothing owns "a draft's
provider identity is being (re)established". Every guard is a workaround for
the same missing object: **an authority for the stable→live mapping that is
fresh in all regimes.**

---

## 2. The clean model

### 2.1 D135 — ONE authority: the `draft_registry`

**Decision: promote the alias into a `draft_registry` table that is the single
authoritative store of the stable→live mapping, written through one port by
*all* writers — including sync. Demote `message.draft_id` to a pure read-model
column (kept for the UI/list rows, D131/M63 unchanged), never consulted for
resolution. Delete the `resolve_draft_entity` fallback.**

Evaluated alternatives:

- **Projection-as-authority, alias eliminated** (the "why isn't the message
  table enough" option). Rejected: (a) an offline/unflushed save has no
  projection row — the draft's optimistic existence lives in the outbox by
  design (`domain-service outbox.rs:345-347`), so resolution would have a
  hole exactly where drafts are born; (b) an in-session rotation would
  require rewriting `message.id` (a PK that fans out to
  `conversation_message`, locations, attachments) at flush time, ahead of
  sync — heavyweight and racing the sync writer it is trying to pre-empt.
  The projection is a *read model*; making it the write-side authority
  inverts its role.
- **Alias-as-authority with sync write-through, no rename.** This is 90% of
  the decision; the rename to `draft_registry` (plus an `updated_at` column)
  is taken because the table's contract changes from "this runtime's session
  cache" to "the durable identity registry", and code reading `draft_alias`
  should stop compiling. Cheap, honest.

Schema (migrated from `draft_alias`, `sql.rs:156-161`):

```sql
CREATE TABLE draft_registry (
    account_id     TEXT NOT NULL,
    draft_key      TEXT NOT NULL,   -- the stable X-Posthaste-Draft-Id value
    live_entity_id TEXT NOT NULL,   -- temp key pre-flush; provider id after
    updated_at     TEXT NOT NULL,
    PRIMARY KEY (account_id, draft_key)
);
CREATE INDEX idx_draft_registry_entity ON draft_registry (account_id, live_entity_id);
```

**The sync write-through is the load-bearing addition.** The store's message
upsert already receives `draft_id` (it writes the column); the same write —
inside the same transaction — upserts `draft_registry(K → message.id)` when
the observation is new. The prune path mirrors it: when sync deletes a message
row whose id the registry points at, the registry repoints if another
projected row carries the same `draft_key` (rotation observed: new row synced,
old destroyed) and **forgets only when no projected row carries the key and no
unsettled draft op references it** (confirmed gone). This closes the §1.2
staleness matrix: the flush loop keeps the registry fresh in-session; sync
keeps it fresh for everything else; there is no regime with two answers, so
there is no precedence `if`. Both writers go through the same SQLite writer
connection the store already serializes on, so the registry cannot be torn
between them.

### 2.2 D136 — ONE seam: the `DraftRegistry` port, resolve-at-flush

**Decision: extract the draft-identity methods out of `OperationOutboxStore`
(`ports/write_store.rs:151-178`) into a dedicated port, and make draft outbox
ops carry the STABLE key as their entity id, resolved to the live id at push
time.**

```rust
/// Durable authority for a draft's identity: the stable client key and the
/// live entity (temp or provider id) currently embodying it.
pub trait DraftRegistry: Send + Sync {
    /// Total: Some(live ref) iff the key names a draft not yet confirmed
    /// destroyed. ONE SELECT; no fallback.
    fn resolve(&self, account: &AccountId, key: &DraftKey)
        -> Result<Option<LiveDraftRef>, StoreError>;
    /// Birth or re-point (idempotent upsert).
    fn register(&self, account: &AccountId, key: &DraftKey, live: &str)
        -> Result<(), StoreError>;
    /// A provider rotation: whatever key maps to `from` now maps to `to`.
    fn rotate(&self, account: &AccountId, from: &str, to: &str)
        -> Result<(), StoreError>;
    /// Confirmed destruction ONLY (settlement/sync-confirmed) — never at enqueue.
    fn forget(&self, account: &AccountId, key: &DraftKey)
        -> Result<(), StoreError>;
}

pub struct LiveDraftRef {
    pub entity_id: MessageId,
    /// Pre-first-flush (entity is the temp key) vs provider-confirmed.
    pub liveness: DraftLiveness, // Local | Provider
}
```

The behavioral shift that dissolves the special cases: **draft ops stop
carrying a snapshot of the live id.** `DraftUpdate`/`DraftDelete` operations
are keyed by `entity_id = stable key`; `push_operation`
(`outbox.rs:830-879`) resolves key→live via the port immediately before the
gateway call. Consequences, mechanically:

- The `resolve_draft_entity` if-fallback (`store/outbox.rs:344-356`) —
  **deleted**; resolution is one SELECT against the registry.
- The dual rewrite at flush reconcile (`outbox.rs:630-641`) — collapses to
  one `registry.rotate(old, new)`; `reconcile_operation_entity_id` no longer
  applies to draft ops at all (their entity id is the key, which never
  rotates). The `Transient`-arm hole (DS2's "re-queues without reconciling",
  `outbox.rs:690-705`) becomes structurally irrelevant to identity: a
  re-queued op still carries the key, and re-resolves fresh next pass.
- `delete_draft`'s enqueue-time `remove_draft_alias` (`outbox.rs:428`) —
  **deleted**; `forget` happens at destroy *settlement* (the `DraftDelete`
  Applied arm, next to the existing D132 event at `outbox.rs:658-669`) or at
  sync-confirmed disappearance (§2.1). Identity is never forgotten before the
  fact is true.
- The four independent liveness probes (§1.3 items 1–4) become one:
  `resolve(key).is_some()`. `draft_message_exists` (`outbox.rs:511-521`) is
  deleted; the "resumed by rotating provider id / legacy draft" bootstrap in
  `save_draft` (`outbox.rs:373-382`) survives as a one-time
  registry-bootstrap query (projection row exists with this id/key but no
  registry row → `register` then proceed), inside the registry adapter — not
  in the domain service.
- `consume_draft_after_send` (`outbox.rs:778-809`) and `discard_draft`
  (`outbox.rs:445-504`) share the identical prologue: `match
  registry.resolve(key)` — `Some` ⇒ enqueue destroy-by-key; `None` ⇒
  send-consume: done (already consumed — idempotency by construction);
  discard: surface `NotFound` (D133's user-facing half, preserved, now one
  line instead of a mailbox-membership probe).

### 2.3 D137 — resolve-at-flush retires the `notFound` discrimination lattice

With resolution guaranteed maximally fresh at dispatch (D136) and the registry
sync-maintained (D135), a provider `notFound` on a draft destroy has exactly
one meaning: *the draft is already gone* (destroyed by another device between
our sync and our flush — an inherently unavoidable race, and a benign one:
the destroy's goal state already holds). **Decision: `notFound` on a draft
destroy (standalone or the DS3 replace-destroy) is uniformly
success-as-already-gone, followed by `forget` + the D132 deleted event.**

What this deletes:

- the `idempotent_redelivery` payload flag + its re-parse
  (`outbox.rs:426,871-875`),
- the parameter on both `MailGateway` methods
  (`ports/gateway.rs:190-196,212-216`) and both adapters,
- the JMAP masks' conditionals (`live_compose/draft.rs:122-133,167-176`
  become unconditional already-gone handling),
- the **incorrect** `attempts > 0` redelivery heuristic (`outbox.rs:852-857`).

Why this does *not* resurrect the M60 silent-discard regression (the whole
reason D133 exists): that bug was a **stale id** — the client discarded a
rotated-away Email id, the provider truthfully said `notFound`, and the mask
lied "success" while the live draft survived (RFC-L2-drafts §"Field bug"; the
"entity ops are not folded into message reads" note now at
`domain-service outbox.rs:644-645`). Under D135/D136 a stale id at dispatch is impossible by
construction (freshest mapping, resolved at push); `notFound` therefore no
longer has a "the live draft survived under another id" reading — if the
draft *does* live under another id, sync repoints the registry and the D138
convergence sweep (below) handles the leftover, rather than a per-call mask
guessing. D133's *user-surfacing* half is preserved where it belongs: at
discard **enqueue**, `resolve → None` still errors (§2.2).

### 2.4 D138 — the anti-twin invariant, owned by the registry

**Decision: invariant I-TWIN — at most one live provider draft per stable
key; the registry names it; any observed violator converges by destroying the
elder.** The sync write-through (§2.1) is the detection point: when sync
projects a *second* row carrying `draft_id = K` (the DS2 lost-response twin;
a DS3 survivor; any race), the registry repoints to the newest observation
and enqueues a `DraftDelete` (by key-with-explicit-target, an internal form)
for the elder row. This turns the twin from a permanent artifact requiring
per-path prevention heroics into a self-healing violation of a stated
invariant — while D139 (below) makes the common cause not produce one at all.

### 2.5 D139 — identity birth is first-class (subsumes the in-flight DS2 create-id)

**Decision: the stable key is minted once per compose session (client side —
aligning with DS4's per-compose-session Idempotency-Key; server side as
today's `draft-local-<uuid>` fallback, `outbox.rs:361-363`); the registry row
is born at save-enqueue (`register(key, key)` — today's self-alias,
`outbox.rs:372`, kept but renamed to what it is); and the provider-side birth
is idempotent: the JMAP create id is derived deterministically from the
operation id (`phdraft-<op-id>`, mirroring `send.rs:19,97`), and a
`DraftCreate` re-flush with `attempts > 0` first attempts adoption — query
the provider by the `X-Posthaste-Draft-Id` header (JMAP `Email/query` header
filter; IMAP: the location store already registered at save,
`gateway/draft.rs:120-158`) and, on a hit, `rotate` to the found id instead
of re-creating.** The in-flight DS2 create-id work lands as-is and is then
recognized as this decision's first half; the adopt-by-header retry is the
second half. Birth, rotation, and death are now the three verbs of one
object's lifecycle, not three unrelated code sites.

### 2.6 D140 — provider specifics stay in the adapters

No new gateway surface is needed; the port *shrinks* (D137 drops a parameter
from both draft methods). The division of labor, stated so it stays stated:

- **Domain service** knows only: keys, the `DraftRegistry` port, and that
  `MailGateway::save_draft` returns the (possibly new) live id. It performs
  `rotate` on that return (`outbox.rs:627-643` successor).
- **JMAP adapter** owns: create+destroy-in-one-`Email/set` rotation
  (`live_compose/draft.rs:35-99`), deterministic create ids (D139), header
  stamping.
- **IMAP adapter** owns: APPEND/UID mechanics, the sync-identical projection
  registration (`gateway/draft.rs:120-158` — unchanged; it is this RFC's
  model done right, a year early), location-store-based deletion
  (`gateway/draft.rs:177-220`).

The registry adapter (the `DraftRegistry` impl on `DatabaseStore`) is
provider-agnostic — it maps keys to whatever id shape the adapter returned.

### 2.7 D141 — "why not resolve a draft like any other email" — the honest answer

**Shared** (and this RFC widens the sharing): a draft *is* a message row —
same table, same summaries/details/list queries, same destroy machinery
(`discard_draft` deliberately mirrors `destroy_message`, `outbox.rs:475-487`),
same event topics, same outbox discipline. Everything *downstream of
resolution* is common code, and stays so.

**Necessarily different**: every other message's provider id is stable for
its lifetime, so *id = identity* and resolution is the identity function. A
draft's provider id rotates on every save on both providers (§1.1), so its
identity must live one level up: a stable handle plus exactly one mapping to
the current id. The system already accepts this principle for the narrow
pre-first-flush window of any entity (temp-id reconciliation,
`ports/write_store.rs:134-141`); a draft is simply an entity that never
leaves that regime. One registry, one port, one extra SELECT at dispatch —
that is the entire, principled difference. Anything beyond it (the fallback,
the flags, the probes) was accident, and this RFC deletes it.

---

## 3. What gets DELETED (the point of the exercise)

| Artifact | Where | Fate |
|---|---|---|
| The alias-then-projection fallback | `store/outbox.rs:344-356` | deleted (M69) — resolution is one registry SELECT |
| `draft_alias` table + entity index | `sql.rs:156-161,302-303` | migrated/renamed to `draft_registry` (M68/M73) |
| Alias methods on `OperationOutboxStore` | `ports/write_store.rs:151-178` | moved to the `DraftRegistry` port (M68); dual flush rewrite (`outbox.rs:637-641`) collapses to `rotate` (M70) |
| Enqueue-time `remove_draft_alias` | `domain outbox.rs:428` | deleted (M70) — `forget` at settlement/sync confirmation only |
| `draft_message_exists` probe + its three call sites | `outbox.rs:378,511-521,803` | deleted (M70) — `resolve` is the one liveness question |
| Discard's mailbox+summary liveness probe | `outbox.rs:459-472` | replaced by `resolve → None ⇒ NotFound` (M70) |
| `idempotent_redelivery`: payload flag, re-parse, gateway params, JMAP mask conditionals, `attempts>0` heuristic | `outbox.rs:426,852-857,871-875`; `ports/gateway.rs:190-196,212-216`; `live_compose/draft.rs:122-133,167-176` | deleted (M71) — uniform already-gone semantics under flush-fresh resolution |
| Anonymous JMAP draft create | `live_compose/draft.rs:38` | replaced by deterministic `phdraft-<op-id>` + adopt-by-header retry (M72; the in-flight DS2 fix is the first half) |
| Draft ops carrying rotating ids | `outbox.rs:369-370,415-423` | ops carry the stable key; resolve-at-flush (M70) |

**Kept, explicitly:** `message.draft_id` column + M63 row surfacing (read
model, D131's UI half); the IMAP save-time location registration (M62/D128 —
already the right model); D125 lifecycle, D126 send-consume-as-settlement,
D127/D134 discard UX, D132 reconciling events; the parked-send-keeps-draft
rule; the DS3 `destroyed(replace)` inspection (simplified by D137, not
removed — a *rejected* replace-destroy still surfaces).

---

## 4. Migration (each step shippable + tested; sequence respects the in-flight [WIP:send-save] worker)

| Step | Scope | Gate | Invariants held |
|---|---|---|---|
| **M68** | Mechanical extraction: `DraftRegistry` port (D136 shape) carved out of `OperationOutboxStore`; impl backed by the existing `draft_alias` table; `resolve` keeps the projection fallback *internally* (behavior-identical). No caller outside the impl touches alias SQL. | full suite green; grep gate: no `draft_alias` reference outside the registry impl + schema | pure refactor; all D125–D134 behavior byte-identical |
| **M69** | D135 sync write-through: message upsert/prune maintains the registry in-transaction (repoint on observation; forget on confirmed-gone-with-no-unsettled-op); THEN delete the resolve fallback. | new tests: other-device rotation (sync-observed) resolves fresh; restart + resolve; offline-save resolve (registry, no projection row); the Stalwart discard repro (`testkit stalwart_draft_discard.rs`) still green | no mail loss (forget only on confirmed gone); discard/send-consume unchanged |
| **M70** | D136 resolve-at-flush: draft ops carry the stable key; `push_operation` resolves via the port; collapse the dual reconcile to `rotate`; move `forget` to `DraftDelete` settlement; delete `draft_message_exists`, the discard probe, the enqueue-time alias removal; `consume_draft_after_send`/`discard_draft` share the one prologue. | e2e per provider: save→rotate→discard hits the LIVE id; send-consume after N autosaves destroys the live draft; discard of a never-saved key surfaces NotFound; permanent-destroy-failure then re-save does not twin | D126 idempotent redelivery (resolve→None ⇒ no-op); D133 user-surfacing at enqueue; no twins |
| **M71** | D137: uniform `notFound ⇒ already-gone` on draft destroys; delete the flag lattice (both ports, both adapters, payload, heuristic). | regression suite incl. the M60 field-bug repro (must stay fixed — via freshness, not the mask); DS3 test: rejected (non-notFound) replace-destroy still surfaces | discard correctness under the other-device race (converges, never lies) |
| **M72** | D139 birth: deterministic `phdraft-<op-id>` create id (**subsumes the in-flight DS2 create-id — coordinate: land theirs first, then absorb**) + adopt-by-header on `DraftCreate` re-flush; client mints the key per compose session (DS4 alignment). | lost-response retry test: kill after commit, re-flush → ONE draft, registry points at it | DS2 twin closed for good; ruling-24 idempotency |
| **M73** | Cleanup: schema migration `draft_alias`→`draft_registry` (+`updated_at`); D138 convergence sweep (second-row-same-key ⇒ repoint + destroy elder); docs cross-links (RFC-L2-drafts Part 2 marked completed-by-this-RFC; BETA-READINESS DS2/DS3 rows pointed here). | migration test on a dogfood-shaped DB; injected-twin test self-heals to one row | I-TWIN stated + enforced |

Sequencing: M68→M69 are safe now and touch no [WIP:send-save] files'
semantics; M70–M71 are one worker, sequential (both live in
`domain-service/service/outbox.rs`, the same surface as the in-flight M65/M66
send-bridge — **do not parallelize with it; queue behind it**); M72
explicitly absorbs the in-flight DS2 patch rather than racing it; M73 last.

Invariants that must hold at every intermediate step: no draft loss (a
mapping is never forgotten before its destruction is confirmed — new,
stronger than today); no twins (and after M73, self-healing); a parked
`DispatchUncertain` send keeps its draft (D125 — untouched, verified by the
existing gate); send-consume idempotent under settlement redelivery (ruling
24); discard never silently succeeds against a surviving draft (the M60
lesson — protected by freshness after M71, by D133 before it).

## 5. Out of scope (deliberately)

- The M66 async-settlement→mutation bridge and M67 WS multi-method hang
  (RFC-L2-drafts refinement section) — orthogonal transport/settlement work.
- Any change to the optimistic fold vocabulary (`MessageAssertion`) or the
  replica/entity-store client — this RFC ends at the domain-service/store/
  adapter boundary.
- Send identity (`phsend-…`, DispatchUncertain parking) — already sound
  (RFC-L2-provider-reliability M32); D139 copies its pattern, does not touch it.
- DS6 (stranded Drafts copy on failed submission) — a lifecycle bug, not an
  identity bug; it should get simpler on top of this model but is its own fix.
- The `message.draft_id` read-model column, list-row surfacing, compose UI.
