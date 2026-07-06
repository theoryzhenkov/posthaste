/**
 * PARAMETERIZED actions (the action-registry completion).
 *
 * Covers `message.move-to-mailbox` end to end at the registry level plus each
 * surface's picker plumbing:
 *  - `resolveParams` returns the account's mailboxes minus the message's
 *    current memberships and non-movable roles;
 *  - the CONTEXT MENU surface resolves the action with `params` +
 *    `executeWith` (what MessageRow renders as the "Move to ▸" submenu);
 *  - the PALETTE two-step: the root provider maps the action to an
 *    `open-action-params` entry (stays open), the pick-step provider lists the
 *    options filtered by the typed query, and running a picked option
 *    dispatches `email.moveToMailbox` with the chosen mailbox;
 *  - the KEYBOARD chord (`m`) resolves the action and routes to the picker
 *    opener (never a silent no-op, never a bare run);
 *  - `message.snooze` is parameterized the same way (preset options).
 */
import { describe, expect, it, mock } from 'bun:test'
import { renderHook } from '@testing-library/react'

import '../src/actions/defs/message'
import '../src/actions/defs/app'
import {
  resolveActions,
  resolveKeyboardAction,
  runResolvedWithConfirm,
  type ChordEvent,
} from '../src/actions'
import type {
  ActionContext,
  ActionServices,
  MessageTarget,
} from '../src/actions'
import { createActionProvider } from '../src/command-search/providers/actions'
import { createActionParamProvider } from '../src/command-search/providers/actionParams'
import { usePaletteActions } from '../src/components/command-palette/usePaletteActions'
import { createRankingContext } from '../src/components/command-palette/model'
import type { Mailbox, MessageSummary } from '../src/api/types'
import type { ProviderSearchRequest } from '../src/command-search/types'
import { SYSTEM_KEYWORDS } from '../src/domainVocabulary'
import type { EmailActions } from '../src/hooks/useEmailActions'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

function makeSpyActions() {
  return {
    toggleRead: mock(() => {}),
    toggleFlag: mock(() => {}),
    archive: mock(() => {}),
    moveToInbox: mock(() => {}),
    moveToMailbox: mock(() => {}),
    trash: mock(() => {}),
    deletePermanently: mock(() => {}),
    discardDraft: mock(() => {}),
    snooze: mock(() => {}),
    isPending: false,
  }
}

function mailbox(over: Partial<Mailbox> & Pick<Mailbox, 'id'>): Mailbox {
  return {
    name: over.id,
    role: null,
    unreadEmails: 0,
    totalEmails: 0,
    ...over,
  }
}

/** A realistic account mailbox set: roles + user folders. The message lives in
 *  `mb-inbox`, so that one must be excluded from the options. */
const MAILBOXES: Mailbox[] = [
  mailbox({ id: 'mb-inbox', name: 'Inbox', role: 'inbox' }),
  mailbox({ id: 'mb-archive', name: 'Archive', role: 'archive' }),
  mailbox({ id: 'mb-drafts', name: 'Drafts', role: 'drafts' }),
  mailbox({ id: 'mb-sent', name: 'Sent', role: 'sent' }),
  mailbox({ id: 'mb-junk', name: 'Junk', role: 'junk' }),
  mailbox({ id: 'mb-trash', name: 'Trash', role: 'trash' }),
  mailbox({ id: 'mb-snooze', name: 'Snoozed', role: 'snooze' }),
  mailbox({ id: 'mb-receipts', name: 'Receipts' }),
  mailbox({ id: 'mb-travel', name: 'Travel' }),
]

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
    mailboxIds: ['mb-inbox'],
    keywords: [],
    draftId: null,
    ...over,
  }
}

function target(over: Partial<MessageSummary> = {}): MessageTarget {
  const s = summary(over)
  return {
    ref: { sourceId: s.sourceId, messageId: s.id },
    summary: s,
    isDraft: s.keywords.includes(SYSTEM_KEYWORDS.Draft),
    draftId: s.draftId,
    conversationId: s.conversationId,
  }
}

function services(email = makeSpyActions()): ActionServices & {
  spy: ReturnType<typeof makeSpyActions>
} {
  return {
    email: email as unknown as EmailActions,
    mailboxes: { list: (sourceId) => (sourceId === 's1' ? MAILBOXES : []) },
    spy: email,
  }
}

function ctx(over: Partial<ActionContext> = {}): ActionContext {
  return {
    targets: [target()],
    viewRole: 'inbox',
    activePane: 'list',
    surface: 'context-menu',
    inputOwner: 'mail',
    hasPendingMutation: false,
    connection: 'unknown',
    ...over,
  }
}

function findMove(context: ActionContext, s: ActionServices) {
  return resolveActions(context, s).find(
    (r) => r.def.id === 'message.move-to-mailbox',
  )
}

describe('message.move-to-mailbox — resolveParams', () => {
  it('lists the account mailboxes minus current membership and non-movable roles', () => {
    const move = findMove(ctx(), services())
    expect(move).toBeDefined()
    expect(move?.params?.map((p) => p.id)).toEqual([
      'mb-archive',
      'mb-junk',
      'mb-receipts',
      'mb-travel',
    ])
    expect(move?.params?.map((p) => p.label)).toEqual([
      'Archive',
      'Junk',
      'Receipts',
      'Travel',
    ])
  })

  it('is hidden without a mailbox source, for drafts, and when nothing is pickable', () => {
    // No `services.mailboxes` (e.g. the parity harness) → not offered.
    const email = makeSpyActions()
    expect(
      findMove(ctx(), { email: email as unknown as EmailActions }),
    ).toBeUndefined()
    // A draft is never moved (discard semantics own it).
    expect(
      findMove(
        ctx({ targets: [target({ keywords: [SYSTEM_KEYWORDS.Draft] })] }),
        services(),
      ),
    ).toBeUndefined()
    // Every candidate excluded → zero options → dropped, not an empty submenu.
    const everywhere = target({
      mailboxIds: MAILBOXES.map((mb) => mb.id),
    })
    expect(findMove(ctx({ targets: [everywhere] }), services())).toBeUndefined()
  })

  it('executeWith moves the message via email.moveToMailbox (context submenu path)', () => {
    const s = services()
    const move = findMove(ctx(), s)!
    const travel = move.params!.find((p) => p.id === 'mb-travel')!
    void move.executeWith?.(travel)
    expect(s.spy.moveToMailbox).toHaveBeenCalledWith(
      { sourceId: 's1', messageId: 'm1' },
      'mb-travel',
      'Travel',
    )
  })
})

function request(query: string): ProviderSearchRequest {
  return {
    query,
    limit: 50,
    context: createRankingContext({ hasSelectedMessage: true }),
  }
}

describe('palette two-step flow', () => {
  it('the root provider maps a parameterized action to a stay-open pick-step opener', async () => {
    const s = services()
    const provider = createActionProvider({
      getContext: () => ctx({ surface: 'palette', inputOwner: 'overlay' }),
      getServices: () => s,
    })
    const page = await provider.search(request('move'))
    const entry = page.candidates
      .map((c) => c.entry)
      .find((e) => e.id === 'message.move-to-mailbox')
    expect(entry?.action).toEqual({
      kind: 'open-action-params',
      actionId: 'message.move-to-mailbox',
    })
    expect(entry?.closeOnSelect).toBe(false)
    // Snooze is parameterized too.
    const snoozePage = await provider.search(request('snooze'))
    expect(snoozePage.candidates.map((c) => c.entry.action.kind)).toContain(
      'open-action-params',
    )
  })

  it('the pick-step provider lists the options and search filters them', async () => {
    const s = services()
    const provider = createActionParamProvider({
      actionId: 'message.move-to-mailbox',
      label: 'Move to…',
      getContext: () => ctx({ surface: 'palette', inputOwner: 'overlay' }),
      getServices: () => s,
    })
    const all = await provider.search(request(''))
    expect(all.candidates.map((c) => c.entry.label)).toEqual([
      'Archive',
      'Junk',
      'Receipts',
      'Travel',
    ])
    const filtered = await provider.search(request('tra'))
    expect(filtered.candidates.map((c) => c.entry.label)).toEqual(['Travel'])
    const chosen = filtered.candidates[0].entry.action
    expect(chosen.kind).toBe('run-action-param')
  })

  it('running a picked option dispatches the move with that mailbox', () => {
    const s = services()
    const paletteCtx = ctx({ surface: 'palette', inputOwner: 'overlay' })
    const openActionParams = mock(() => {})
    const { result } = renderHook(() =>
      usePaletteActions({
        actionContext: paletteCtx,
        services: s,
        nav: {
          onApplySearch: () => {},
          onSelectMessage: () => {},
          onSelectSmartMailbox: () => {},
          onSelectSourceMailbox: () => {},
          replaceQuery: () => {},
          openActionParams,
        },
      }),
    )
    // Step 1: selecting the parameterized command opens its pick-step.
    result.current({
      kind: 'open-action-params',
      actionId: 'message.move-to-mailbox',
    })
    expect(openActionParams).toHaveBeenCalledWith('message.move-to-mailbox')
    expect(s.spy.moveToMailbox).not.toHaveBeenCalled()
    // Step 2: the picked option runs the move.
    result.current({
      kind: 'run-action-param',
      actionId: 'message.move-to-mailbox',
      param: { id: 'mb-receipts', label: 'Receipts' },
    })
    expect(s.spy.moveToMailbox).toHaveBeenCalledWith(
      { sourceId: 's1', messageId: 'm1' },
      'mb-receipts',
      'Receipts',
    )
  })
})

const M_CHORD: ChordEvent = {
  key: 'm',
  metaKey: false,
  ctrlKey: false,
  shiftKey: false,
  altKey: false,
}

describe('keyboard chord → picker (never a silent no-op)', () => {
  it('`m` resolves to move-to-mailbox and routes to the param picker', () => {
    const s = services()
    const keyboardCtx = ctx({ surface: 'keyboard' })
    const resolved = resolveKeyboardAction(M_CHORD, keyboardCtx, s)
    expect(resolved?.def.id).toBe('message.move-to-mailbox')

    const requestConfirm = mock(() => {})
    const requestParam = mock(() => {})
    runResolvedWithConfirm(resolved!, requestConfirm, requestParam)
    // The picker was requested; nothing ran and nothing asked for confirm.
    expect(requestParam).toHaveBeenCalledTimes(1)
    expect(requestConfirm).not.toHaveBeenCalled()
    expect(s.spy.moveToMailbox).not.toHaveBeenCalled()
  })

  it('`m` stays unbound without a mailbox source or a target', () => {
    const email = makeSpyActions()
    expect(
      resolveKeyboardAction(M_CHORD, ctx({ surface: 'keyboard' }), {
        email: email as unknown as EmailActions,
      }),
    ).toBeNull()
    expect(
      resolveKeyboardAction(
        M_CHORD,
        ctx({ surface: 'keyboard', targets: [] }),
        services(),
      ),
    ).toBeNull()
  })
})

describe('message.snooze — parameterized presets', () => {
  it('resolves preset options on the palette and runs email.snooze with the pick', () => {
    const s = services()
    const snooze = resolveActions(
      ctx({ surface: 'palette', inputOwner: 'overlay' }),
      s,
    ).find((r) => r.def.id === 'message.snooze')
    expect(snooze?.params?.length).toBeGreaterThan(0)
    const tomorrow = snooze!.params!.find((p) => p.label === 'Tomorrow')!
    void snooze!.executeWith?.(tomorrow)
    expect(s.spy.snooze).toHaveBeenCalledWith(
      { sourceId: 's1', messageId: 'm1' },
      Number(tomorrow.id),
    )
  })
})
