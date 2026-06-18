/**
 * Invertible mail operations.
 *
 * Every mutation (archive, trash, move, flag, delete) is modelled as a
 * projection from a target's current {@link MutableState} to a desired next
 * state. Running an operation records the per-target before/after pair as an
 * {@link AppliedOperation}; undo is then a single generic primitive —
 * {@link invertOperation} projects every target back to its captured before
 * image — rather than hardcoded per-operation logic. `destroy` is the one
 * irreversible kind and is reported as non-invertible.
 *
 * This module is framework-free (no React, no query client) so the inverse
 * logic can be unit-tested in isolation.
 *
 * @spec docs/L1-ui#undo-system
 */
import type { KnownMailboxRole, SourceMessageRef } from './api/types'
import type { MutableState } from './mailState'

/** Which facet of a message an operation changes. */
export type OperationKind =
  | 'keywords'
  | 'mailboxes'
  | 'mailbox-role'
  | 'destroy'

/** A single message target, with its conversation id when known. */
export type OperationTarget = SourceMessageRef & { conversationId?: string }

// Built-in operations change a single facet (mailboxes OR keywords). The runner
// and rollback snapshots assume this; a combined-facet projection is expressible
// but would need snapshot dedupe per query key before it is introduced.

/**
 * A mutation to run: which targets, how to project each target's current state
 * to its desired next state, and the labels used for the toast / undo affordance.
 */
export interface MailOperation {
  kind: OperationKind
  targets: OperationTarget[]
  /** Runtime-owned mailbox role intent for `mailbox-role` operations. */
  mailboxRole?: KnownMailboxRole
  /** Project a target's current state to its desired next state when local. */
  project?: (target: OperationTarget, current: MutableState) => MutableState
  /** Toast copy shown after the operation runs (e.g. "Message archived"). */
  label: string
  /** Undo-button copy; defaults to "Undo". */
  undoLabel?: string
}

/** The before/after state recorded for one target once an operation is applied. */
export interface OperationEntry {
  target: OperationTarget
  before: MutableState
  after: MutableState
}

/**
 * A mutation that has been applied, holding enough state to invert (or replay)
 * it. Pushed onto the undo history by the runner.
 */
export interface AppliedOperation {
  kind: OperationKind
  label: string
  undoLabel?: string
  entries: OperationEntry[]
  /** False for irreversible operations (destroy). */
  invertible: boolean
}

/** Stable key for matching a target across an operation and its inverse. */
export function targetKey(target: SourceMessageRef): string {
  return `${target.sourceId}:${target.messageId}`
}

function projectToRecorded(
  applied: AppliedOperation,
  facet: 'before' | 'after',
): NonNullable<MailOperation['project']> {
  const byKey = new Map(
    applied.entries.map((entry) => [targetKey(entry.target), entry[facet]]),
  )
  return (target, current) => byKey.get(targetKey(target)) ?? current
}

/**
 * Build the operation that returns every target to its captured before-image.
 * Returns null for irreversible operations (destroy).
 * @spec docs/L1-ui#undo-system
 */
export function invertOperation(
  applied: AppliedOperation,
): MailOperation | null {
  if (!applied.invertible) {
    return null
  }
  return {
    kind: applied.kind,
    targets: applied.entries.map((entry) => entry.target),
    project: projectToRecorded(applied, 'before'),
    label: 'Change reverted',
  }
}

/**
 * Build the operation that re-applies the captured after-image — used for redo.
 */
export function replayOperation(applied: AppliedOperation): MailOperation {
  return {
    kind: applied.kind,
    targets: applied.entries.map((entry) => entry.target),
    project: projectToRecorded(applied, 'after'),
    label: applied.label,
    undoLabel: applied.undoLabel,
  }
}

function applyKeywordDelta(
  keywords: string[],
  add: string[],
  remove: string[],
): string[] {
  const removeSet = new Set(remove)
  const next = keywords.filter((keyword) => !removeSet.has(keyword))
  for (const keyword of add) {
    if (!next.includes(keyword)) {
      next.push(keyword)
    }
  }
  return next
}

/** Move the target into exactly the given mailbox (archive / trash / restore). */
export function moveToMailboxOp(
  target: OperationTarget,
  mailboxId: string,
  label: string,
  undoLabel?: string,
): MailOperation {
  return {
    kind: 'mailboxes',
    targets: [target],
    project: (_target, current) => ({ ...current, mailboxIds: [mailboxId] }),
    label,
    undoLabel,
  }
}

/** Move the target by runtime mailbox-role intent; the runner resolves legacy ids. */
export function moveToMailboxRoleOp(
  target: OperationTarget,
  role: KnownMailboxRole,
  label: string,
  undoLabel?: string,
): MailOperation {
  return {
    kind: 'mailbox-role',
    mailboxRole: role,
    targets: [target],
    label,
    undoLabel,
  }
}

/**
 * Apply a keyword add/remove delta (read, flag, tags) relative to live state.
 * Keyword operations are never toasted, so they carry no user-facing label.
 */
export function setKeywordsOp(
  target: OperationTarget,
  delta: { add: string[]; remove: string[] },
): MailOperation {
  return {
    kind: 'keywords',
    targets: [target],
    project: (_target, current) => ({
      ...current,
      keywords: applyKeywordDelta(current.keywords, delta.add, delta.remove),
    }),
    label: '',
  }
}

/** Permanently delete the target. Irreversible: not pushed to the undo stack. */
export function destroyOp(
  target: OperationTarget,
  label: string,
): MailOperation {
  return {
    kind: 'destroy',
    targets: [target],
    project: (_target, current) => current,
    label,
  }
}
