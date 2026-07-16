/**
 * PLAN-L2 Slice 5 — the keyboard tier resolves mail-action chords CONTEXTUALLY.
 *
 * Proves the owner bug is fixed at the resolver level: the SAME `#`/Backspace
 * chord resolves to `move-to-trash` outside Trash and `delete-permanently`
 * inside it, that at most one available action ever claims the chord, and that
 * the destructive delete routes through a confirm gate instead of running from
 * the bare keystroke.
 */
import { describe, expect, it, mock } from 'bun:test'

import '../src/actions/defs/message'
import '../src/actions/defs/app'
import {
  resolveKeyboardAction,
  runResolvedWithConfirm,
  matchesChord,
  shortcutMatches,
  type ChordEvent,
} from '../src/actions/keyboard'
import { resolveActions } from '../src/actions/resolve'
import type { ActionContext, MessageTarget } from '../src/actions/types'
import type { MessageSummary, SourceMessageRef } from '../src/api/types'
import { SYSTEM_KEYWORDS } from '../src/domainVocabulary'
import type { EmailActions } from '../src/hooks/useEmailActions'

function makeSpyActions() {
  return {
    toggleRead: mock(() => {}),
    toggleFlag: mock(() => {}),
    archive: mock(() => {}),
    moveToInbox: mock(() => {}),
    trash: mock(() => {}),
    deletePermanently: mock(() => {}),
    discardDraft: mock(() => {}),
    isPending: false,
  }
}

const REF: SourceMessageRef = { sourceId: 's1', messageId: 'm1' }

function summary(over: Partial<MessageSummary> = {}): MessageSummary {
  return {
    id: 'm1',
    sourceId: 's1',
    sourceName: 'Acct',
    sourceThreadId: 't1',
    conversationId: 'c1',
    subject: 'Hi',
    fromName: 'A',
    fromEmail: 'a@x.test',
    to: [],
    preview: null,
    receivedAt: '2026-01-01T00:00:00Z',
    hasAttachment: false,
    isRead: false,
    isFlagged: false,
    mailboxIds: ['mb1'],
    keywords: [],
    draftId: null,
    ...over,
  }
}

function target(over: Partial<MessageTarget> = {}): MessageTarget {
  const s = over.summary ?? summary()
  return {
    ref: REF,
    summary: s,
    isDraft: s.keywords.includes(SYSTEM_KEYWORDS.Draft),
    draftId: s.draftId,
    conversationId: s.conversationId,
    ...over,
  }
}

function ctx(over: Partial<ActionContext> = {}): ActionContext {
  return {
    targets: [target()],
    viewRole: 'inbox',
    activePane: 'list',
    surface: 'keyboard',
    inputOwner: 'mail',
    hasPendingMutation: false,
    connection: 'unknown',
    ...over,
  }
}

const HASH: ChordEvent = {
  key: '#',
  metaKey: false,
  ctrlKey: false,
  shiftKey: true, // '#' is Shift+3 — a bare { key: '#' } chord must still match
  altKey: false,
}
const BACKSPACE: ChordEvent = {
  key: 'Backspace',
  metaKey: false,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
}
const E: ChordEvent = { ...BACKSPACE, key: 'e' }

describe('resolveKeyboardAction — the #/Backspace chord is contextual', () => {
  // spec: docs/eph/PLAN-L2-action-registry.md
  it('resolves to move-to-trash OUTSIDE trash', () => {
    const services = { email: makeSpyActions() as unknown as EmailActions }
    for (const viewRole of [null, 'inbox', 'archive', 'sent']) {
      const hit = resolveKeyboardAction(HASH, ctx({ viewRole }), services)
      expect(hit?.def.id).toBe('message.move-to-trash')
      const back = resolveKeyboardAction(BACKSPACE, ctx({ viewRole }), services)
      expect(back?.def.id).toBe('message.move-to-trash')
    }
  })

  it('resolves to delete-permanently INSIDE trash (the owner bug)', () => {
    const services = { email: makeSpyActions() as unknown as EmailActions }
    const hit = resolveKeyboardAction(
      HASH,
      ctx({ viewRole: 'trash' }),
      services,
    )
    expect(hit?.def.id).toBe('message.delete-permanently')
    const back = resolveKeyboardAction(
      BACKSPACE,
      ctx({ viewRole: 'trash' }),
      services,
    )
    expect(back?.def.id).toBe('message.delete-permanently')
  })

  it('never lets more than one available action claim the chord', () => {
    const services = { email: makeSpyActions() as unknown as EmailActions }
    for (const viewRole of [null, 'inbox', 'archive', 'trash', 'junk']) {
      const matches = resolveActions(ctx({ viewRole }), services).filter((r) =>
        shortcutMatches(r.def.shortcut, HASH),
      )
      expect(matches.length).toBeLessThanOrEqual(1)
    }
  })

  it('a draft falls through (no move-to-trash / delete) so native discard runs', () => {
    const services = { email: makeSpyActions() as unknown as EmailActions }
    const draft = target({
      summary: summary({ keywords: [SYSTEM_KEYWORDS.Draft] }),
    })
    expect(
      resolveKeyboardAction(
        HASH,
        ctx({ viewRole: 'inbox', targets: [draft] }),
        services,
      ),
    ).toBeNull()
    expect(
      resolveKeyboardAction(
        HASH,
        ctx({ viewRole: 'trash', targets: [draft] }),
        services,
      ),
    ).toBeNull()
  })

  it('archive `e` resolves to archive (and is unavailable in trash)', () => {
    const services = { email: makeSpyActions() as unknown as EmailActions }
    expect(
      resolveKeyboardAction(E, ctx({ viewRole: 'inbox' }), services)?.def.id,
    ).toBe('message.archive')
    expect(
      resolveKeyboardAction(E, ctx({ viewRole: 'trash' }), services),
    ).toBeNull()
  })

  it('returns null with no target (bare keystroke never fires against nothing)', () => {
    const services = { email: makeSpyActions() as unknown as EmailActions }
    expect(
      resolveKeyboardAction(HASH, ctx({ targets: [] }), services),
    ).toBeNull()
  })
})

describe('matchesChord — shifted symbols and named keys', () => {
  it('matches a bare `#` chord even though Shift is held', () => {
    expect(matchesChord({ key: '#' }, HASH)).toBe(true)
  })
  it('matches Backspace case-insensitively', () => {
    expect(matchesChord({ key: 'backspace' }, BACKSPACE)).toBe(true)
  })
  it('enforces mod strictly', () => {
    expect(matchesChord({ key: 'e' }, { ...E, metaKey: true })).toBe(false)
  })
})

describe('runResolvedWithConfirm — destructive keyboard actions prompt', () => {
  it('delete-permanently PROMPTS and only runs after confirm (no silent delete)', () => {
    const email = makeSpyActions()
    const services = { email: email as unknown as EmailActions }
    const resolved = resolveKeyboardAction(
      HASH,
      ctx({ viewRole: 'trash' }),
      services,
    )!
    expect(resolved.def.id).toBe('message.delete-permanently')

    let captured: (() => void) | null = null
    const requestConfirm = mock(
      (_c: { title: string }, onConfirm: () => void) => {
        captured = onConfirm
      },
    )
    runResolvedWithConfirm(resolved, requestConfirm)

    // Prompted, but NOT yet executed.
    expect(requestConfirm).toHaveBeenCalledTimes(1)
    expect(email.deletePermanently).not.toHaveBeenCalled()

    // Accepting the dialog runs the irreversible delete.
    captured!()
    expect(email.deletePermanently).toHaveBeenCalledWith(REF)
  })

  it('move-to-trash runs INSTANTLY (reversible — no prompt)', () => {
    const email = makeSpyActions()
    const services = { email: email as unknown as EmailActions }
    const resolved = resolveKeyboardAction(
      HASH,
      ctx({ viewRole: 'inbox' }),
      services,
    )!
    const requestConfirm = mock(() => {})
    runResolvedWithConfirm(resolved, requestConfirm)
    expect(requestConfirm).not.toHaveBeenCalled()
    expect(email.trash).toHaveBeenCalledWith(REF)
  })
})
