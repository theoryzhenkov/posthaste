import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  jest,
  spyOn,
} from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook } from '@testing-library/react'
import type { ReactNode } from 'react'
import * as sonner from 'sonner'

import type { SourceMessageRef } from '../src/api/types'
import {
  DRAFT_DISCARD_GRACE_MS,
  useEmailActions,
} from '../src/hooks/useEmailActions'
import { runtimeMutations } from '../src/runtime/mutations'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const draft: SourceMessageRef & { draftId?: string | null } = {
  sourceId: 'acct-1',
  messageId: 'draft-9',
  draftId: 'compose-session-9',
}

function makeWrapper(qc: QueryClient) {
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={qc}>{children}</QueryClientProvider>
  )
}

function renderActions() {
  const qc = new QueryClient()
  return renderHook(() => useEmailActions({ undo: () => {} }), {
    wrapper: makeWrapper(qc),
  })
}

/** Pull the Undo callback the discard toast registered. */
function undoFromLastToast(toastSpy: ReturnType<typeof spyOn>): () => void {
  const options = toastSpy.mock.calls.at(-1)?.[1] as
    | { action?: { onClick?: () => void } }
    | undefined
  const onClick = options?.action?.onClick
  if (!onClick) {
    throw new Error('discard toast did not register an Undo action')
  }
  return onClick
}

describe('useEmailActions.discardDraft (D127)', () => {
  beforeEach(() => {
    jest.useFakeTimers()
  })
  afterEach(() => {
    jest.useRealTimers()
  })

  it('folds the discard optimistically IMMEDIATELY on click, then dispatches the server destroy only after the grace (never the trash mutation)', async () => {
    const foldSpy = spyOn(
      runtimeMutations.messages,
      'foldDiscard',
    ).mockResolvedValue('discard_fold_id' as never)
    const discardSpy = spyOn(
      runtimeMutations.messages,
      'discardDraft',
    ).mockResolvedValue({} as never)
    const trashSpy = spyOn(
      runtimeMutations.messages,
      'moveToMailboxRole',
    ).mockResolvedValue({} as never)
    const toastSpy = spyOn(sonner, 'toast').mockReturnValue('id' as never)

    try {
      const { result } = renderActions()

      act(() => {
        result.current.discardDraft(draft)
      })

      // FIX1: the optimistic fold (the blink) fires SYNCHRONOUSLY on click —
      // the row is removed immediately, not after the grace.
      expect(foldSpy).toHaveBeenCalledTimes(1)
      const foldId = foldSpy.mock.calls[0]?.[0]?.clientMutationId as string
      expect(foldId).toBeTruthy()
      expect(foldSpy).toHaveBeenCalledWith({
        sourceId: 'acct-1',
        messageId: 'draft-9',
        draftId: 'compose-session-9',
        clientMutationId: foldId,
      })
      // Nothing dispatched to the server during the grace window.
      expect(discardSpy).not.toHaveBeenCalled()

      await act(async () => {
        jest.advanceTimersByTime(DRAFT_DISCARD_GRACE_MS)
        await Promise.resolve()
      })

      expect(discardSpy).toHaveBeenCalledTimes(1)
      // D131 + FIX1: the commit re-runs the SAME mutation under the fold's id
      // (idempotent re-fold, no second blink); the stable draftId rides along.
      expect(discardSpy).toHaveBeenCalledWith(
        {
          sourceId: 'acct-1',
          messageId: 'draft-9',
          draftId: 'compose-session-9',
          clientMutationId: foldId,
        },
        { userInitiated: true },
      )
      // The regression guard: a draft must never hit the trash move.
      expect(trashSpy).not.toHaveBeenCalled()
    } finally {
      foldSpy.mockRestore()
      discardSpy.mockRestore()
      trashSpy.mockRestore()
      toastSpy.mockRestore()
    }
  })

  it('Undo within the grace reverts the folded row and NEVER dispatches to the server', async () => {
    const foldSpy = spyOn(
      runtimeMutations.messages,
      'foldDiscard',
    ).mockResolvedValue('discard_fold_id' as never)
    const revertSpy = spyOn(
      runtimeMutations.messages,
      'revertDiscard',
    ).mockResolvedValue(undefined as never)
    const discardSpy = spyOn(
      runtimeMutations.messages,
      'discardDraft',
    ).mockResolvedValue({} as never)
    const toastSpy = spyOn(sonner, 'toast').mockReturnValue('id' as never)

    try {
      const { result } = renderActions()

      act(() => {
        result.current.discardDraft(draft)
      })

      const foldId = foldSpy.mock.calls[0]?.[0]?.clientMutationId as string

      // Press Undo before the grace elapses.
      act(() => {
        undoFromLastToast(toastSpy)()
      })

      // The folded row is restored client-side (same id), with no round-trip.
      expect(revertSpy).toHaveBeenCalledTimes(1)
      expect(revertSpy).toHaveBeenCalledWith(foldId)

      await act(async () => {
        jest.advanceTimersByTime(DRAFT_DISCARD_GRACE_MS * 2)
        await Promise.resolve()
      })

      // Nothing was ever dispatched to the server.
      expect(discardSpy).not.toHaveBeenCalled()
    } finally {
      foldSpy.mockRestore()
      revertSpy.mockRestore()
      discardSpy.mockRestore()
      toastSpy.mockRestore()
    }
  })

  it('coalesces repeated discards of the same draft into one fold + one dispatch', async () => {
    const foldSpy = spyOn(
      runtimeMutations.messages,
      'foldDiscard',
    ).mockResolvedValue('discard_fold_id' as never)
    const discardSpy = spyOn(
      runtimeMutations.messages,
      'discardDraft',
    ).mockResolvedValue({} as never)
    const toastSpy = spyOn(sonner, 'toast').mockReturnValue('id' as never)

    try {
      const { result } = renderActions()

      act(() => {
        result.current.discardDraft(draft)
        result.current.discardDraft(draft)
      })

      // The second click is coalesced: exactly one fold (one blink).
      expect(foldSpy).toHaveBeenCalledTimes(1)

      await act(async () => {
        jest.advanceTimersByTime(DRAFT_DISCARD_GRACE_MS)
        await Promise.resolve()
      })

      expect(discardSpy).toHaveBeenCalledTimes(1)
    } finally {
      foldSpy.mockRestore()
      discardSpy.mockRestore()
      toastSpy.mockRestore()
    }
  })
})
