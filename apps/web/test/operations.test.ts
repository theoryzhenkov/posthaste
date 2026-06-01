import { describe, expect, it } from 'bun:test'

import { diffMutableState, type MutableState } from '../src/mailState'
import {
  destroyOp,
  invertOperation,
  moveToMailboxOp,
  replayOperation,
  setKeywordsOp,
  targetKey,
  type AppliedOperation,
  type MailOperation,
  type OperationTarget,
} from '../src/operations'

const target: OperationTarget = {
  sourceId: 'primary',
  messageId: 'm1',
  conversationId: 'c1',
}

/** Simulate the runner: project each target's before-image into an applied op. */
function apply(
  operation: MailOperation,
  befores: Record<string, MutableState>,
): AppliedOperation {
  const entries = operation.targets.map((entryTarget) => {
    const before = befores[targetKey(entryTarget)]
    const after =
      operation.kind === 'destroy'
        ? before
        : operation.project(entryTarget, before)
    return { after, before, target: entryTarget }
  })
  return {
    entries,
    invertible: operation.kind !== 'destroy',
    kind: operation.kind,
    label: operation.label,
    undoLabel: operation.undoLabel,
  }
}

describe('invertOperation', () => {
  it('restores a trashed message to its original mailbox, not the inbox', () => {
    // The reported bug: trash a message that lives in Archive, then undo.
    const operation = moveToMailboxOp(target, 'trash', 'Message trashed')
    const applied = apply(operation, {
      [targetKey(target)]: { keywords: [], mailboxIds: ['archive'] },
    })

    expect(applied.entries[0].after.mailboxIds).toEqual(['trash'])

    const inverse = invertOperation(applied)
    expect(inverse).not.toBeNull()

    const restored = inverse!.project(target, applied.entries[0].after)
    expect(restored.mailboxIds).toEqual(['archive'])

    // The command the runner would issue to undo points back at Archive.
    const commands = diffMutableState(applied.entries[0].after, restored)
    expect(commands).toEqual([
      { kind: 'replaceMailboxes', mailboxIds: ['archive'] },
    ])
  })

  it('returns null for irreversible destroy operations', () => {
    const applied = apply(destroyOp(target, 'Permanently deleted'), {
      [targetKey(target)]: { keywords: ['$seen'], mailboxIds: ['trash'] },
    })
    expect(applied.invertible).toBe(false)
    expect(invertOperation(applied)).toBeNull()
  })

  it('inverts a keyword toggle back to the prior keyword set', () => {
    const operation = setKeywordsOp(target, { add: ['$flagged'], remove: [] })
    const applied = apply(operation, {
      [targetKey(target)]: { keywords: ['$seen'], mailboxIds: ['inbox'] },
    })
    expect(applied.entries[0].after.keywords).toEqual(['$seen', '$flagged'])

    const inverse = invertOperation(applied)!
    const restored = inverse.project(target, applied.entries[0].after)
    expect(diffMutableState(applied.entries[0].after, restored)).toEqual([
      { kind: 'setKeywords', add: [], remove: ['$flagged'] },
    ])
  })

  it('maps each target to its own captured before-image', () => {
    const second: OperationTarget = { ...target, messageId: 'm2' }
    const operation: MailOperation = {
      kind: 'mailboxes',
      label: 'Moved',
      targets: [target, second],
      project: (_t, current) => ({ ...current, mailboxIds: ['trash'] }),
    }
    const applied = apply(operation, {
      [targetKey(target)]: { keywords: [], mailboxIds: ['inbox'] },
      [targetKey(second)]: { keywords: [], mailboxIds: ['archive'] },
    })
    const inverse = invertOperation(applied)!
    expect(
      inverse.project(target, { keywords: [], mailboxIds: ['trash'] }),
    ).toEqual({ keywords: [], mailboxIds: ['inbox'] })
    expect(
      inverse.project(second, { keywords: [], mailboxIds: ['trash'] }),
    ).toEqual({ keywords: [], mailboxIds: ['archive'] })
  })
})

describe('replayOperation', () => {
  it('re-applies the captured after-image (redo)', () => {
    const operation = moveToMailboxOp(target, 'archive', 'Message archived')
    const applied = apply(operation, {
      [targetKey(target)]: { keywords: [], mailboxIds: ['inbox'] },
    })
    const replay = replayOperation(applied)
    expect(
      replay.project(target, { keywords: [], mailboxIds: ['inbox'] }),
    ).toEqual({ keywords: [], mailboxIds: ['archive'] })
  })
})

describe('diffMutableState', () => {
  it('emits no commands when state is unchanged', () => {
    const state: MutableState = { keywords: ['$seen'], mailboxIds: ['inbox'] }
    expect(diffMutableState(state, { ...state })).toEqual([])
  })

  it('ignores mailbox ordering differences', () => {
    expect(
      diffMutableState(
        { keywords: [], mailboxIds: ['a', 'b'] },
        { keywords: [], mailboxIds: ['b', 'a'] },
      ),
    ).toEqual([])
  })

  it('emits a combined keyword delta', () => {
    expect(
      diffMutableState(
        { keywords: ['$seen', '$flagged'], mailboxIds: ['inbox'] },
        { keywords: ['$seen', 'work'], mailboxIds: ['inbox'] },
      ),
    ).toEqual([{ kind: 'setKeywords', add: ['work'], remove: ['$flagged'] }])
  })

  it('orders replaceMailboxes before setKeywords for a combined change', () => {
    // Rollback ordering depends on this: the mailbox command comes first.
    const commands = diffMutableState(
      { keywords: ['$seen'], mailboxIds: ['inbox'] },
      { keywords: [], mailboxIds: ['archive'] },
    )
    expect(commands.map((command) => command.kind)).toEqual([
      'replaceMailboxes',
      'setKeywords',
    ])
  })
})
