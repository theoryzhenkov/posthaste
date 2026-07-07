/**
 * The surface-coverage AUDIT, encoded (action-registry completion).
 *
 * One assertion per registered action pinning exactly which surfaces it opts
 * into — the reviewed coverage matrix. Changing an action's reach is a
 * deliberate act: update this table alongside the definition.
 *
 * Not every reachable behavior appears here as a surface: some chords stay in
 * the NATIVE keyboard tier (dispatch.ts) rather than the registry —
 * reply (⌘R), reply-all (⌘⇧R), toggle-flag (⌘⇧L), open-focused (`o`),
 * view-conversation (`g c`) — so their definitions deliberately omit
 * 'keyboard' even though a shortcut exists.
 */
import { describe, expect, it } from 'bun:test'

import '../src/actions/defs/message'
import '../src/actions/defs/app'
import { allActions } from '../src/actions'

/** id → the surfaces it must (exactly) cover. */
const EXPECTED_SURFACES: Record<string, string[]> = {
  // -- open --------------------------------------------------------------
  // Row-open makes no sense in the header (the message is already open).
  'message.open': ['context-menu', 'palette'],
  'message.view-conversation': ['context-menu', 'palette'],
  'message.open-focused': ['palette', 'detail-header'],
  // -- state ---------------------------------------------------------------
  'message.toggle-read': ['context-menu', 'palette', 'keyboard'],
  'message.toggle-flag': ['context-menu', 'palette', 'detail-header'],
  // -- compose / reply ------------------------------------------------------
  'message.reply': ['palette', 'detail-header'],
  'message.reply-all': ['palette', 'detail-header'],
  'message.forward': ['palette', 'detail-header'],
  'message.edit-draft': ['palette', 'detail-header'],
  // -- move ------------------------------------------------------------------
  'message.archive': ['context-menu', 'palette', 'keyboard', 'detail-header'],
  'message.move-to-inbox': ['context-menu', 'palette', 'detail-header'],
  'message.move-to-mailbox': ['context-menu', 'palette', 'keyboard'],
  'message.move-to-trash': [
    'context-menu',
    'palette',
    'keyboard',
    'detail-header',
  ],
  'message.delete-permanently': [
    'context-menu',
    'palette',
    'keyboard',
    'detail-header',
  ],
  'message.discard-draft': ['context-menu', 'palette', 'detail-header'],
  // -- organize ---------------------------------------------------------------
  'message.tag': ['palette', 'keyboard', 'detail-header'],
  'message.snooze': ['palette', 'detail-header'],
  // List-Unsubscribe: data-gated (the detail DTO's parsed targets) AND
  // capability-gated (`services.unsubscribe`, bound only by confirm-honoring
  // hosts — the detail header today), so listing menu/palette here does not
  // light them up until a host binds the service there.
  'message.unsubscribe': ['context-menu', 'palette', 'detail-header'],
  // -- app-level (palette-only global commands) -------------------------------
  'app.compose': ['palette'],
  'app.new-smart-mailbox': ['palette'],
  'app.new-rule': ['palette'],
  'app.manage-tags': ['palette'],
  'app.open-settings': ['palette'],
  'app.shortcuts': ['palette'],
  'app.add-account': ['palette'],
}

describe('action surface coverage (the audited matrix)', () => {
  it('every registered action covers exactly its audited surfaces', () => {
    const actual = Object.fromEntries(
      allActions().map((def) => [def.id, [...def.surfaces]]),
    )
    expect(actual).toEqual(EXPECTED_SURFACES)
  })

  it('parameterized actions are exactly move-to-mailbox and snooze', () => {
    const parameterized = allActions()
      .filter((def) => def.resolveParams !== undefined)
      .map((def) => def.id)
      .sort()
    expect(parameterized).toEqual(['message.move-to-mailbox', 'message.snooze'])
  })
})
