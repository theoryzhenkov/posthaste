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

  it('dispatches the optimistic discard mutation after the grace with the stable draft id and NEVER the trash mutation', async () => {
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

      // Nothing dispatched during the grace window.
      expect(discardSpy).not.toHaveBeenCalled()

      await act(async () => {
        jest.advanceTimersByTime(DRAFT_DISCARD_GRACE_MS)
        await Promise.resolve()
      })

      expect(discardSpy).toHaveBeenCalledTimes(1)
      // D131: the optimistic fold keys on the row's messageId (the blink); the
      // stable draftId rides along so the far node resolves the live Email.
      expect(discardSpy).toHaveBeenCalledWith(
        {
          sourceId: 'acct-1',
          messageId: 'draft-9',
          draftId: 'compose-session-9',
        },
        { userInitiated: true },
      )
      // The regression guard: a draft must never hit the trash move.
      expect(trashSpy).not.toHaveBeenCalled()
    } finally {
      discardSpy.mockRestore()
      trashSpy.mockRestore()
      toastSpy.mockRestore()
    }
  })

  it('cancels the dispatch when Undo is pressed within the grace', async () => {
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

      // Press Undo before the grace elapses.
      act(() => {
        undoFromLastToast(toastSpy)()
      })

      await act(async () => {
        jest.advanceTimersByTime(DRAFT_DISCARD_GRACE_MS * 2)
        await Promise.resolve()
      })

      expect(discardSpy).not.toHaveBeenCalled()
    } finally {
      discardSpy.mockRestore()
      toastSpy.mockRestore()
    }
  })

  it('coalesces repeated discards of the same draft into one dispatch', async () => {
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

      await act(async () => {
        jest.advanceTimersByTime(DRAFT_DISCARD_GRACE_MS)
        await Promise.resolve()
      })

      expect(discardSpy).toHaveBeenCalledTimes(1)
    } finally {
      discardSpy.mockRestore()
      toastSpy.mockRestore()
    }
  })
})
