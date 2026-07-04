# RFC-L2-drafts — the draft lifecycle, done properly

> **Status (2026-07-04): SHIPPED.** Ratified (owner, 2026-07-04) and fully
> landed: M60 (send consumes the draft — settlement effect, both providers, incl.
> the Gmail Sent-copy rider), M61 (discard = hard-delete + the edit-draft
> action-row icon), M62 (idempotent draft saves + the canonical-id twin fix).
> **[Update 2026-07-04]:** the execution notes below ("M61 launched; M60 queued;
> M62 after M60") are historical — all three steps landed.

Status: RATIFIED (owner, 2026-07-04). Execution: M61 launched immediately; M60 queued behind the in-flight archive fix (shared IMAP gateway surface); M62 after M60. Owner reports: sending a
draft leaves it in Drafts; deleting a draft lands it in Trash; edit-draft is an
ad-hoc button. Survey evidence below; each mechanism confirmed in code.

## 1. How real clients handle drafts (the reference model)

- **Identity**: one compose session ↔ one logical draft. Autosave REPLACES the
  server draft (new version supersedes; on IMAP the UID changes each save, on
  JMAP the Email is updated/recreated). The local compose buffer is the source
  of truth while editing.
- **Send consumes the draft**: on a successful submission the draft is
  destroyed and the sent copy appears in Sent. JMAP encodes this atomically —
  `EmailSubmission` carries `onSuccessDestroyEmail`/`onSuccessUpdateEmail`
  (RFC 8621 §7) so the server does the cleanup in the same transaction. IMAP
  clients delete the draft (mark `\Deleted` + expunge in Drafts) after the SMTP
  submit acks.
- **Discard ≠ trash**: Gmail's "discard draft" hard-deletes (no Trash hop);
  this is the modern UX norm. An undo affordance is a short CLIENT-side grace
  before dispatch, not a Trash round-trip.
- **Gmail provider gotchas**: mail sent via Gmail SMTP is auto-placed in Sent
  by Gmail — a client that also APPENDs to Sent creates the classic duplicate;
  Gmail's Drafts folder is label-backed like everything else.

## 2. Current state (surveyed 2026-07-04)

| Area | State | Evidence |
|---|---|---|
| Send→draft linkage | `SendMessageRequest.draft_id: Option<String>` EXISTS on the wire — but grep shows **no consumer**: nothing destroys the draft on send settlement. The symptom is exactly this missing half. | `commands.rs:286`; no consumer in domain-service/runtime/engine |
| Draft delete | A correct hard-delete path exists (`delete_imap_draft` → per-location expunge) — but the UI's delete on a draft routes through the **generic trash mutation** instead. | `gateway/draft.rs:103`; UI routing to confirm in M61 |
| Save/versioning | Append-then-delete replace semantics EXIST (old kept if append fails — good). Wart: the re-appended draft can transiently duplicate under a new canonical id until sync reconciles (doc'd as "acceptable for the JMAP-first beta"). | `gateway/draft.rs:21-38` |
| Idempotency | Draft routes have NO apply-ledger — a replayed save duplicates; a proper fix must return the SAME operation id (ruling 24 flag). | RFC-L2-scripting §25 |
| UI | Edit-draft is an ad-hoc button, not in the standard message-action row. | owner report |

## 3. Decisions (proposed)

- **D125 — The draft lifecycle contract.** One compose session ↔ one draft
  identity. States: `Editing (local)` → `Saved(vN)` (server draft, replaced
  per save) → `Sending` → on SETTLED success `Consumed` (draft destroyed, sent
  copy in Sent) / on `DispatchUncertain` **the draft is KEPT** — it is the
  user's recovery artifact and pairs with the M32 park UI ("may not have
  sent" + the draft still openable). No state may strand both a parked send
  and a lost draft.
- **D126 — Send consumes the draft.** The already-carried `draft_id` gains its
  consumer: draft destruction is a **settlement effect** of the send operation
  (idempotent, survives redelivery — same discipline as every settlement).
  JMAP: attach `onSuccessDestroyEmail` to the EmailSubmission (atomic,
  server-side; verify fork support — M32's `create_with_id` work suggests the
  fork is capable). IMAP: after the submit acks, expunge the draft from Drafts
  (reuse the UID EXPUNGE helper from the archive fix). Failure to clean up is
  retried at next settlement/sync, never silent.
- **D127 — Discard is hard delete.** Deleting a draft routes to the
  draft-delete op (expunge / JMAP destroy), NEVER the trash mutation. A short
  client-side undo grace (delay dispatch ~5s with an undo toast) replaces the
  Trash safety net.
- **D128 — Save is replace, idempotent, identity-stable.** Keep
  append-then-delete; add the Idempotency-Key ledger to save/delete-draft with
  same-operation-id replay (closes the ruling-24 flag); kill the
  canonical-id wart: the save path knows the new UID (APPENDUID) — register
  the location→canonical-id mapping at save time so sync cannot create a
  transient twin.
- **D129 — Edit-draft joins the message-action row** as an icon (shown for
  `\Draft`-flagged messages), replacing the ad-hoc button; the row's action
  set for drafts is draft-appropriate (edit, discard — not trash).
- **Gmail rider — no double Sent copy.** Verify the send path does not APPEND
  to Sent on providers that auto-place (Gmail SMTP); gate per-provider.

## 4. Migration

| Step | Scope | Gate |
|---|---|---|
| M60 | D126 send-consumes-draft: settlement effect in the runtime + both gateways (JMAP onSuccessDestroyEmail; IMAP expunge-after-ack) + the DispatchUncertain draft-kept rule + the Gmail Sent-copy check | e2e: send a draft → draft gone from Drafts, one copy in Sent (per provider incl. Gmail fixture); parked send keeps the draft |
| M61 | D127 discard: UI routing to the draft-delete op + undo grace; D129 the action-row icon | UI tests: draft delete never calls trash; icon appears only on drafts; undo restores |
| M62 | D128: apply-ledger on draft routes w/ same-id replay + the canonical-id mapping fix | replayed save = one draft, same op id; save produces no transient twin in the list |

Sequencing: M60 is provider work (Fable per standing rule) and the biggest
user pain; M61 is apps/web (parallel-safe); M62 rides after M60 (shares the
draft-route surface).

## Field bug (2026-07-04, JMAP) — discard/send-consume silently no-op on a stale rotated draft id (M60 REGRESSION)
JMAP drafts are immutable → every autosave edit ROTATES the Email id (create-new+destroy-old, E1→E2→…). The client list row carries the last-SYNCED id (entity draft ops aren't folded into message reads — outbox.rs:563) so discard targets a STALE, already-destroyed id → server answers notFound → M60's D126 masking (engine live_compose/draft.rs:137-146, notFound=>Ok(())) swallows it as SUCCESS → op settles Applied, toast fires, the live draft survives. 'Discard does nothing.' M60 (d303b5b69) is the regression — pre-M60 notFound surfaced as a retryable failure. SAME flaw silently breaks M60's send-consumes-draft (unresolved stable key → notFound → mask → draft lingers in Drafts after send). FIX (opus): (1) narrow the notFound masking to the idempotent/redelivery case ONLY — a user-initiated discard's notFound must surface (retryable), not silent success; (2) resolve the LIVE Email id at delete time — in service.delete_draft, when the supplied key doesn't resolve to a current live draft, look it up by the stable X-Posthaste-Draft-Id header in the projection and destroy THAT; treat as already-gone only when no live draft matches (fixes both stale-rotation discard AND unresolved-stable-key send-consume). Files: engine/live_compose/draft.rs, domain-service/service/outbox.rs (delete_draft/resolve_draft_entity).

## RFC Part 2 — the unified optimistic draft/send lifecycle (owner-ratified 2026-07-04, completes D125)
Root cause CONFIRMED by a real-Stalwart-JMAP reproduction (testkit stalwart_draft_discard.rs): the BACKEND discard is correct (synced id == live Email id; Email/set destroy returns destroyed; sync prunes) — the bug is that discard is a FIRE-AND-FORGET DIRECT command (httpAdapter.deleteDraft POST + a 5s setTimeout + an immediate toast), NOT a runRuntimeMutation, so it has no optimistic fold (no blink, by construction), no settlement/convergence, and no surfaced error — and the deferred POST/prune never reliably reaches the server or prunes the WASM store row. Same class afflicts save-draft and send. Owner ruling: FULL UNIFICATION.

- **D130 — Drafts and sends are OPTIMISTIC ENTITIES routed through runRuntimeMutation**, exactly like tag/move/trash. Discard = an optimistic destroy assertion keyed on the row's messageId → folds instantly (the blink) → settles on the runtime notification / reverts + surfaces the error on failure. Save = an optimistic upsert. Send = an optimistic Sent-row + submission. No more fire-and-forget direct commands for these three. The replica already has the pieces (generic destroy assertion, optimistic fold, settle-on-mutationNotification, prune-on-deleted).
- **D131 — Stable draft identity carried end-to-end.** A draft is keyed by its stable X-Posthaste-Draft-Id (already stamped in the header + projected as MessageDetail.draftId) — SURFACE it on the list row (MessageSummary) too (today only MessageDetail carries it, so discard sends the raw rotating Email id). The gateway maps stable→current-live-Email in ONE place (the existing draft_alias table / resolve_draft_entity). No id-bearing draft op ever carries a rotating id. This is the owner's 'guarantee the id is always correct after rotation' — done centrally, not resolve-at-delete.
- **D132 — Settlement emits the reconciling event.** DraftDelete/save settlement emits message.updated{deleted:true}/upsert so the fold/prune reconciles WITHOUT leaning on a follow-up sync (today it emits only operation.settled).
- **D133 — Narrow the M60 notFound⇒Ok mask** (engine live_compose/draft.rs:137-146) to the idempotent-redelivery case ONLY; a user-initiated discard's notFound surfaces retryably (defensive point-fix, good regardless).
- **D134 — Discard undo becomes a reverting settlement** (optimistic remove; undo reverts the fold) rather than a pre-dispatch 5s grace.

Migration (phased; M64 fixes the reported bug):
| Step | Scope | Gate |
|---|---|---|
| M63 | D131 stable draft id: surface draftId on MessageSummary/list rows + the single draft_alias stable→live mapping | a list draft row carries the stable id; no op carries a rotating Email id |
| M64 | D130+D132+D133+D134 for DISCARD: delete-draft through runRuntimeMutation (optimistic destroy + settle/revert + the blink) + settlement emits deleted + narrow the M60 mask | the Stalwart replication test + the M48 harness: discard optimistically removes, settles, reverts+surfaces on failure; the reported bug fixed |
| M65 | D130 for SAVE-draft through runRuntimeMutation | save is optimistic + settles |
| M66 | D130 for SEND through runRuntimeMutation (optimistic Sent row) | send is optimistic + settles |
| M67 | the WS multi-method save_draft flush HANG (incidental find — live.rs send_request over the shared WS) | a multi-method flush over WS completes |

Sequencing: M63→M64 first (the foundational core + the reported bug); M65/M66 mirror the pattern; M67 (WS hang) is orthogonal, provider-side.
