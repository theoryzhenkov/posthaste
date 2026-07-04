# RFC-L2-drafts — the draft lifecycle, done properly

Status: DRAFT for owner ratification (2026-07-04). Owner reports: sending a
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
