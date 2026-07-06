/**
 * PLAN-L2 Slice 3 — the palette's single execution path.
 *
 * The old parallel execution switch is gone: registry rows dispatch through
 * `getAction(id).run(ctx, services)` (the same path the context menu uses) and
 * navigation rows route to the remaining nav handlers.
 */
import { describe, expect, it, mock } from 'bun:test'
import { renderHook } from '@testing-library/react'

import type { ActionContext, ActionServices } from '../src/actions'
import { usePaletteActions } from '../src/components/command-palette/usePaletteActions'
import type { PaletteNavHandlers } from '../src/components/command-palette/usePaletteActions'
import type { EmailActions } from '../src/hooks/useEmailActions'
import type { useMailClientHandlers } from '../src/app/useMailClientHandlers'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const REF = { sourceId: 's1', messageId: 'm1' }

function makeSetup() {
  const email = {
    archive: mock(() => {}),
    toggleFlag: mock(() => {}),
    isPending: false,
  }
  const app = { handleReply: mock(() => {}) }
  const services: ActionServices = {
    email: email as unknown as EmailActions,
    app: app as unknown as ReturnType<typeof useMailClientHandlers>,
  }
  const actionContext: ActionContext = {
    targets: [
      {
        ref: REF,
        summary: undefined,
        isDraft: false,
        draftId: null,
        conversationId: 'c1',
      },
    ],
    viewRole: 'inbox',
    activePane: 'list',
    surface: 'palette',
    inputOwner: 'overlay',
    hasPendingMutation: false,
    connection: 'unknown',
  }
  const nav: PaletteNavHandlers & {
    applied: string[]
    mailboxes: string[]
  } = {
    applied: [],
    mailboxes: [],
    onApplySearch(query) {
      this.applied.push(query)
    },
    onSelectMessage() {},
    onSelectSmartMailbox(id) {
      this.mailboxes.push(id)
    },
    onSelectSourceMailbox() {},
    replaceQuery() {},
  }
  return { services, email, app, actionContext, nav }
}

describe('usePaletteActions', () => {
  it('dispatches a registry action through its run(ctx, services)', () => {
    const { services, email, actionContext, nav } = makeSetup()
    const { result } = renderHook(() =>
      usePaletteActions({ actionContext, services, nav }),
    )
    result.current({ kind: 'action', actionId: 'message.archive' })
    expect(email.archive).toHaveBeenCalledWith(REF)
  })

  it('delegates app-scoped registry actions to the app services bundle', () => {
    const { services, app, actionContext, nav } = makeSetup()
    const { result } = renderHook(() =>
      usePaletteActions({ actionContext, services, nav }),
    )
    result.current({ kind: 'action', actionId: 'message.reply' })
    expect(app.handleReply).toHaveBeenCalledTimes(1)
  })

  it('ignores unknown action ids', () => {
    const { services, actionContext, nav } = makeSetup()
    const { result } = renderHook(() =>
      usePaletteActions({ actionContext, services, nav }),
    )
    expect(() =>
      result.current({ kind: 'action', actionId: 'does.not.exist' }),
    ).not.toThrow()
  })

  it('routes navigation kinds to the nav handlers', () => {
    const { services, actionContext, nav } = makeSetup()
    const { result } = renderHook(() =>
      usePaletteActions({ actionContext, services, nav }),
    )
    result.current({ kind: 'apply-query', query: 'is:unread' })
    result.current({
      kind: 'open-smart-mailbox',
      smartMailboxId: 'sm1',
      name: 'Unread',
    })
    expect(nav.applied).toEqual(['is:unread'])
    expect(nav.mailboxes).toEqual(['sm1'])
  })
})
