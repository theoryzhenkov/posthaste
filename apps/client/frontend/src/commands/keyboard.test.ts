/**
 * Dispatcher-tier tests: chord matching (incl. `code` chords), keyboard
 * resolution against the real registry (availability gating by context and
 * scope services), and the confirm/param execution gate.
 */
import { describe, expect, test } from 'bun:test'
import {
  matchesChord,
  resolveKeyboardAction,
  runResolvedWithConfirm,
  type ChordEvent,
} from './index'
import type { ActionContext, ActionServices } from './types'

function chordEvent(partial: Partial<ChordEvent> & { key: string }): ChordEvent {
  return {
    metaKey: false,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    ...partial,
  }
}

function keyboardContext(partial?: Partial<ActionContext>): ActionContext {
  return {
    targets: [],
    viewRole: null,
    activePane: 'list',
    surface: 'keyboard',
    inputOwner: 'mail',
    hasPendingMutation: false,
    connection: 'unknown',
    ...partial,
  }
}

const target = {
  ref: { sourceId: 'src-1', messageId: 'msg-1' },
  isDraft: false,
}

describe('matchesChord', () => {
  test('mod and alt are strict; shift only when declared', () => {
    const chord = { key: 'e' }
    expect(matchesChord(chord, chordEvent({ key: 'e' }))).toBe(true)
    expect(matchesChord(chord, chordEvent({ key: 'e', metaKey: true }))).toBe(
      false,
    )
    expect(matchesChord(chord, chordEvent({ key: 'e', altKey: true }))).toBe(
      false,
    )
    // A shifted symbol matches without the chord declaring shift.
    expect(
      matchesChord({ key: '#' }, chordEvent({ key: '#', shiftKey: true })),
    ).toBe(true)
    expect(
      matchesChord(
        { key: 'r', mod: true, shift: true },
        chordEvent({ key: 'R', metaKey: true, shiftKey: true }),
      ),
    ).toBe(true)
  })

  test('a code chord matches the physical key even when `key` is mangled', () => {
    const devtools = { key: 'i', code: 'KeyI', mod: true, alt: true }
    // macOS ⌥I: event.key is a dead key, event.code stays KeyI.
    expect(
      matchesChord(
        devtools,
        chordEvent({ key: 'ˆ', code: 'KeyI', metaKey: true, altKey: true }),
      ),
    ).toBe(true)
    expect(
      matchesChord(
        devtools,
        chordEvent({ key: 'i', code: 'KeyI', metaKey: true }),
      ),
    ).toBe(false)
  })
})

describe('resolveKeyboardAction — availability gating', () => {
  test('surface.close resolves only in a surface scope with a bound host', () => {
    const escape = chordEvent({ key: 'Escape' })
    const services: ActionServices = { surfaceHost: { close: () => {} } }
    const resolved = resolveKeyboardAction(
      escape,
      keyboardContext({ inputOwner: 'surface' }),
      services,
    )
    expect(resolved?.id).toBe('surface.close')
    // No bound host (canClose false) ⇒ falls through.
    expect(
      resolveKeyboardAction(escape, keyboardContext({ inputOwner: 'surface' }), {}),
    ).toBeNull()
    // Host bound but input owned by the mail shell ⇒ falls through.
    expect(resolveKeyboardAction(escape, keyboardContext(), services)).toBeNull()
  })

  test('compose.send resolves on ⌘Enter only while a composer is bound', () => {
    const sent: string[] = []
    const services: ActionServices = {
      compose: { send: () => sent.push('sent') },
    }
    const event = chordEvent({ key: 'Enter', metaKey: true })
    const resolved = resolveKeyboardAction(
      event,
      keyboardContext({ inputOwner: 'overlay' }),
      services,
    )
    expect(resolved?.id).toBe('compose.send')
    void resolved?.execute()
    expect(sent).toEqual(['sent'])
    expect(
      resolveKeyboardAction(event, keyboardContext({ inputOwner: 'overlay' }), {}),
    ).toBeNull()
  })

  test('app.toggle-devtools gates on the lab toggle', () => {
    const toggles: string[] = []
    const event = chordEvent({
      key: 'ˆ',
      code: 'KeyI',
      metaKey: true,
      altKey: true,
    })
    const enabled: ActionServices = {
      desktop: {
        isDeveloperToolsEnabled: () => true,
        toggleDevtools: () => void toggles.push('toggled'),
      },
    }
    const disabled: ActionServices = {
      desktop: {
        isDeveloperToolsEnabled: () => false,
        toggleDevtools: () => void toggles.push('never'),
      },
    }
    const resolved = resolveKeyboardAction(event, keyboardContext(), enabled)
    expect(resolved?.id).toBe('app.toggle-devtools')
    void resolved?.execute()
    expect(toggles).toEqual(['toggled'])
    expect(resolveKeyboardAction(event, keyboardContext(), disabled)).toBeNull()
    expect(resolveKeyboardAction(event, keyboardContext(), {})).toBeNull()
  })

  test('the migrated modifier chords resolve through the table, not a native map', () => {
    const calls: string[] = []
    const app = {
      handleCompose: () => calls.push('compose'),
      handleOpenSettings: () => calls.push('settings'),
      handleToggleShortcuts: () => calls.push('shortcuts'),
      handleSelectMessage: () => {},
      handleSearch: () => {},
      handleOpenFocusedMessage: () => calls.push('open-focused'),
      handleReply: () => calls.push('reply'),
      handleReplyAll: () => calls.push('reply-all'),
      handleForward: () => {},
      handleEditDraft: () => {},
      handleOpenTagEditor: () => {},
    }
    const services: ActionServices = { app }
    const withTarget = keyboardContext({ targets: [target] })

    const cases: Array<[ChordEvent, string, string]> = [
      [chordEvent({ key: 'r', metaKey: true }), 'message.reply', 'reply'],
      [
        chordEvent({ key: 'R', metaKey: true, shiftKey: true }),
        'message.reply-all',
        'reply-all',
      ],
      [chordEvent({ key: 'n', metaKey: true }), 'app.compose', 'compose'],
      [chordEvent({ key: ',', metaKey: true }), 'app.open-settings', 'settings'],
      [
        chordEvent({ key: '?', shiftKey: true }),
        'app.shortcuts',
        'shortcuts',
      ],
      [chordEvent({ key: 'o' }), 'message.open-focused', 'open-focused'],
    ]
    for (const [event, id, effect] of cases) {
      const resolved = resolveKeyboardAction(event, withTarget, services)
      expect(resolved?.id).toBe(id)
      void resolved?.execute()
      expect(calls.at(-1)).toBe(effect)
    }
    // Without the app bundle bound (the dispatcher's scopes) the chords vanish.
    expect(
      resolveKeyboardAction(
        chordEvent({ key: 'n', metaKey: true }),
        withTarget,
        {},
      ),
    ).toBeNull()
    // ⌘R with nothing selected: still CLAIMED (disabled) under includeDisabled,
    // so the dispatcher swallows the chord instead of leaking it to the browser.
    const disabled = resolveKeyboardAction(
      chordEvent({ key: 'r', metaKey: true }),
      keyboardContext(),
      services,
      { includeDisabled: true },
    )
    expect(disabled?.id).toBe('message.reply')
    expect(disabled?.enabled).toBe(false)
  })

  test('the table alone decides availability: `e` has no claimant in Archive/Trash', () => {
    const event = chordEvent({ key: 'e' })
    const inArchive = keyboardContext({ targets: [target], viewRole: 'archive' })
    const inTrash = keyboardContext({ targets: [target], viewRole: 'trash' })
    // Even with includeDisabled the chord resolves to NOTHING — the former
    // native fallback used to archive here anyway; that override is gone.
    expect(
      resolveKeyboardAction(event, inArchive, {}, { includeDisabled: true }),
    ).toBeNull()
    expect(
      resolveKeyboardAction(event, inTrash, {}, { includeDisabled: true }),
    ).toBeNull()
    expect(
      resolveKeyboardAction(event, keyboardContext({ targets: [target] }), {})
        ?.id,
    ).toBe('message.archive')
  })

  test('#/Backspace on a draft resolves discard-draft (the draft policy lives in the defs)', () => {
    const draft = { ...target, isDraft: true, draftId: 'draft-1' }
    const event = chordEvent({ key: 'Backspace' })
    expect(
      resolveKeyboardAction(
        event,
        keyboardContext({ targets: [draft] }),
        {},
      )?.id,
    ).toBe('message.discard-draft')
    // In Trash a draft still discards — delete-permanently excludes drafts.
    expect(
      resolveKeyboardAction(
        event,
        keyboardContext({ targets: [draft], viewRole: 'trash' }),
        {},
      )?.id,
    ).toBe('message.discard-draft')
  })

  test('the shared #/Backspace chord is view-role contextual', () => {
    const event = chordEvent({ key: '#', shiftKey: true })
    const ctxWithTarget = (viewRole: string | null) =>
      keyboardContext({ targets: [target], viewRole })
    const services: ActionServices = {}
    expect(resolveKeyboardAction(event, ctxWithTarget(null), services)?.id).toBe(
      'message.move-to-trash',
    )
    const inTrash = resolveKeyboardAction(
      event,
      ctxWithTarget('trash'),
      services,
    )
    expect(inTrash?.id).toBe('message.delete-permanently')
    // The irreversible action carries its confirm gate.
    expect(inTrash?.confirm?.confirmLabel).toBe('Delete')
    // No target ⇒ disabled ⇒ no keyboard match at all.
    expect(resolveKeyboardAction(event, keyboardContext(), services)).toBeNull()
  })
})

describe('runResolvedWithConfirm', () => {
  test('confirm-bearing actions never run without acceptance', () => {
    const event = chordEvent({ key: 'Backspace' })
    const resolved = resolveKeyboardAction(
      event,
      keyboardContext({ targets: [target], viewRole: 'trash' }),
      {},
    )
    expect(resolved?.id).toBe('message.delete-permanently')
    const calls: string[] = []
    runResolvedWithConfirm(resolved!, (confirm, onConfirm) => {
      calls.push(`asked:${confirm.title}`)
      onConfirm()
    })
    // The runner executed only via the accepted confirm callback (the email
    // service is unbound here, so acceptance is a no-op mutation-wise).
    expect(calls).toEqual(['asked:Delete permanently?'])
  })

  test('parameterized actions route to the param host instead of running', () => {
    const event = chordEvent({ key: 'm' })
    const resolved = resolveKeyboardAction(
      event,
      keyboardContext({ targets: [target] }),
      {
        mailboxes: {
          list: () => [
            { id: 'mb-1', name: 'Receipts', role: null } as never,
          ],
        },
      },
    )
    expect(resolved?.id).toBe('message.move-to-mailbox')
    const requested: string[] = []
    runResolvedWithConfirm(
      resolved!,
      () => requested.push('confirm'),
      (action) => requested.push(`param:${action.def.id}`),
    )
    expect(requested).toEqual(['param:message.move-to-mailbox'])
  })
})
