/**
 * PLAN-L2 Slice 1 — resolver parity harness.
 *
 * Proves the new registry + `resolveActions` produce, for the `'context-menu'`
 * surface, the SAME ordered move/state action set (ids, labels, sections,
 * destructive flags, order, and role/draft gating) that the old
 * `contextualActions.ts` builder produced — and that each action delegates to
 * the same `EmailActions` method with the same argument. Together with the
 * unchanged `contextualActions.test.ts` (which pins the shim's legacy
 * `builtin.*` output, proving the menu is byte-for-byte unchanged), this is the
 * safety net that makes Slices 2-5 safe.
 *
 * This file asserts the CANONICAL registry ids (`message.*`) via `resolveActions`
 * directly; the shim's legacy-id mapping is covered by `contextualActions.test`.
 */
import { describe, expect, it, mock } from 'bun:test'

// Side-effect import: registers the message definitions into the registry (the
// running app registers them the same way, via the MessageRow → shim import).
import '../src/actions/defs/message'
import '../src/actions/defs/app'
import { resolveActions } from '../src/actions/resolve'
import type { ActionContext, MessageTarget } from '../src/actions/types'
import type { MessageSummary, SourceMessageRef } from '../src/api/types'
import { SYSTEM_KEYWORDS } from '../src/domainVocabulary'
import type { EmailActions } from '../src/hooks/useEmailActions'

/** A fresh spy-backed EmailActions with just the methods the ports touch. */
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

function makeMessage(over: Partial<MessageSummary> = {}): MessageSummary {
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

const REF: SourceMessageRef = { sourceId: 's1', messageId: 'm1' }

function target(over: Partial<MessageTarget> = {}): MessageTarget {
  const summary = over.summary ?? makeMessage()
  return {
    ref: REF,
    summary,
    isDraft: summary.keywords.includes(SYSTEM_KEYWORDS.Draft),
    draftId: summary.draftId,
    conversationId: summary.conversationId,
    ...over,
  }
}

function ctx(over: Partial<ActionContext> = {}): ActionContext {
  return {
    targets: [target(over.targets?.[0])],
    viewRole: 'inbox',
    activePane: 'list',
    surface: 'context-menu',
    inputOwner: 'mail',
    hasPendingMutation: false,
    connection: 'unknown',
    ...over,
  }
}

/** Resolve for the context-menu surface with a fresh spy services bundle. */
function resolve(over: Partial<ActionContext> = {}) {
  const services = { email: makeSpyActions() as unknown as EmailActions }
  const resolved = resolveActions(ctx(over), services)
  return { services, resolved, ids: resolved.map((r) => r.def.id) }
}

describe('resolveActions ↔ context-menu parity (Slice 1)', () => {
  // spec: docs/eph/PLAN-L2-action-registry.md
  it('inbox / null-role: toggle-read, toggle-flag, archive, trash', () => {
    const expected = [
      'message.toggle-read',
      'message.toggle-flag',
      'message.archive',
      'message.move-to-trash',
    ]
    for (const viewRole of [null, 'inbox', 'sent']) {
      expect(resolve({ viewRole }).ids).toEqual(expected)
    }
  })

  it('archive view: no archive; move-to-inbox then trash', () => {
    expect(resolve({ viewRole: 'archive' }).ids).toEqual([
      'message.toggle-read',
      'message.toggle-flag',
      'message.move-to-inbox',
      'message.move-to-trash',
    ])
  })

  it('trash view: no archive; move-to-inbox then delete-permanently', () => {
    expect(resolve({ viewRole: 'trash' }).ids).toEqual([
      'message.toggle-read',
      'message.toggle-flag',
      'message.move-to-inbox',
      'message.delete-permanently',
    ])
  })

  it('junk view: archive, move-to-inbox, trash', () => {
    expect(resolve({ viewRole: 'junk' }).ids).toEqual([
      'message.toggle-read',
      'message.toggle-flag',
      'message.archive',
      'message.move-to-inbox',
      'message.move-to-trash',
    ])
  })

  it('draft in a normal view: archive + discard (never trash/delete)', () => {
    const draft = target({
      summary: makeMessage({ keywords: [SYSTEM_KEYWORDS.Draft] }),
    })
    const { ids } = resolve({ viewRole: 'inbox', targets: [draft] })
    expect(ids).toEqual([
      'message.toggle-read',
      'message.toggle-flag',
      'message.archive',
      'message.discard-draft',
    ])
  })

  it('draft in trash/archive: move-to-inbox + discard (never delete/trash)', () => {
    const draft = target({
      summary: makeMessage({ keywords: [SYSTEM_KEYWORDS.Draft] }),
    })
    for (const viewRole of ['trash', 'archive']) {
      expect(resolve({ viewRole, targets: [draft] }).ids).toEqual([
        'message.toggle-read',
        'message.toggle-flag',
        'message.move-to-inbox',
        'message.discard-draft',
      ])
    }
  })

  it('toggle labels + destructive flags flip / match the old builder', () => {
    const seen = target({
      summary: makeMessage({ isRead: true, isFlagged: true }),
    })
    const { resolved } = resolve({ viewRole: 'trash', targets: [seen] })
    const by = new Map(resolved.map((r) => [r.def.id, r]))
    expect(by.get('message.toggle-read')?.title).toBe('Mark unread')
    expect(by.get('message.toggle-flag')?.title).toBe('Unflag')
    // fresh (unread/unflagged) message flips the other way
    const fresh = resolve({ viewRole: 'inbox' }).resolved
    const byFresh = new Map(fresh.map((r) => [r.def.id, r]))
    expect(byFresh.get('message.toggle-read')?.title).toBe('Mark read')
    expect(byFresh.get('message.toggle-flag')?.title).toBe('Flag')
    expect(byFresh.get('message.archive')?.def.destructive).toBeUndefined()
    expect(byFresh.get('message.move-to-trash')?.def.destructive).toBe(true)
  })
})

describe('resolveActions — delegation to EmailActions (Slice 1)', () => {
  it('each action calls the matching service method with the same argument', () => {
    // archive / trash / toggles from inbox
    const inbox = resolve({ viewRole: 'inbox' })
    const email = inbox.services.email as unknown as ReturnType<
      typeof makeSpyActions
    >
    const run = (id: string) =>
      inbox.resolved.find((r) => r.def.id === id)?.execute()

    run('message.archive')
    expect(email.archive).toHaveBeenCalledWith(REF)
    run('message.move-to-trash')
    expect(email.trash).toHaveBeenCalledWith(REF)
    run('message.toggle-read')
    expect(email.toggleRead.mock.calls[0]?.[0]).toMatchObject({ id: 'm1' })
    run('message.toggle-flag')
    expect(email.toggleFlag.mock.calls[0]?.[0]).toMatchObject({ id: 'm1' })

    // delete-permanently + move-to-inbox from trash
    const trash = resolve({ viewRole: 'trash' })
    const trashEmail = trash.services.email as unknown as ReturnType<
      typeof makeSpyActions
    >
    trash.resolved
      .find((r) => r.def.id === 'message.delete-permanently')
      ?.execute()
    expect(trashEmail.deletePermanently).toHaveBeenCalledWith(REF)
    trash.resolved.find((r) => r.def.id === 'message.move-to-inbox')?.execute()
    expect(trashEmail.moveToInbox).toHaveBeenCalledWith(REF)

    // discard-draft carries the stable draftId (D131)
    const draft = target({
      summary: makeMessage({
        keywords: [SYSTEM_KEYWORDS.Draft],
        draftId: 'd1',
      }),
    })
    const drafted = resolve({ viewRole: 'inbox', targets: [draft] })
    const draftEmail = drafted.services.email as unknown as ReturnType<
      typeof makeSpyActions
    >
    drafted.resolved
      .find((r) => r.def.id === 'message.discard-draft')
      ?.execute()
    expect(draftEmail.discardDraft).toHaveBeenCalledWith({
      sourceId: 's1',
      messageId: 'm1',
      draftId: 'd1',
    })
  })
})

describe('resolveActions — surface + enablement (Slice 1/3)', () => {
  it('palette surface resolves palette-eligible actions; keyboard/detail-header stay empty until later slices', () => {
    // Slice 3 lit up the palette surface: message actions gained 'palette' and
    // the app-level commands (defs/app.ts) register palette-only entries.
    const palette = resolveActions(ctx({ surface: 'palette' }), {
      email: makeSpyActions() as unknown as EmailActions,
    })
    const paletteIds = palette.map((r) => r.def.id)
    expect(paletteIds).toContain('message.archive')
    expect(paletteIds).toContain('message.reply')
    expect(paletteIds).toContain('app.compose')
    // Context-menu-only entries never leak into the palette.
    expect(paletteIds).not.toContain('message.open')

    // The keyboard + detail-header surfaces are migrated in Slices 4-5.
    expect(
      resolveActions(ctx({ surface: 'keyboard' }), {
        email: makeSpyActions() as unknown as EmailActions,
      }),
    ).toHaveLength(0)
    expect(
      resolveActions(ctx({ surface: 'detail-header' }), {
        email: makeSpyActions() as unknown as EmailActions,
      }),
    ).toHaveLength(0)
  })

  it('drops no-target actions from a menu but keeps them disabled with includeDisabled', () => {
    const services = { email: makeSpyActions() as unknown as EmailActions }
    const empty = ctx({ targets: [] })
    expect(resolveActions(empty, services)).toHaveLength(0)
    const withDisabled = resolveActions(empty, services, {
      includeDisabled: true,
    })
    expect(withDisabled.length).toBeGreaterThan(0)
    expect(withDisabled.every((r) => !r.enabled)).toBe(true)
    expect(withDisabled[0]?.disabledReason).toBe('Select a message first')
  })
})
