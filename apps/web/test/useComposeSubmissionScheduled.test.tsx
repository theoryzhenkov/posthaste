/**
 * Undo-send / send-later at the composer boundary: the submit path schedules
 * the send with `sendAt` (one mechanism), persists the draft first so undo
 * restores the full compose, offers Undo while the outbox holds the send, and
 * keeps the pre-feature immediate path byte-identical when the delay is 0.
 */
import { afterEach, describe, expect, it, spyOn } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { act, renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'
import * as sonner from 'sonner'

import type { AppSettings, SendMessageResponse } from '../src/api/types'
import {
  EMPTY_FORM,
  type ComposeForm,
} from '../src/components/composeFormHelpers'
import { useComposeSubmission } from '../src/components/compose-overlay/useComposeSubmission'
import { queryKeys } from '../src/queryKeys'
import { runtimeMutations } from '../src/runtime/mutations'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const SOURCE_ID = 'acct-1'
const DRAFT_KEY = 'draft-local-test-key'

type AnySpy = { mockRestore: () => void }
const activeSpies: AnySpy[] = []

function track<T extends AnySpy>(spy: T): T {
  activeSpies.push(spy)
  return spy
}

afterEach(() => {
  while (activeSpies.length > 0) {
    activeSpies.pop()?.mockRestore()
  }
})

function validForm(): ComposeForm {
  return {
    ...EMPTY_FORM,
    from: 'me@example.test',
    to: 'you@example.test',
    subject: 'Hello',
    body: 'A message body',
  }
}

function settings(undoSendDelaySeconds: number | null): AppSettings {
  return {
    defaultAccountId: null,
    cachePolicy: {
      softCapBytes: 0,
      hardCapBytes: 0,
      cacheBodies: false,
      cacheRawMessages: false,
      cacheAttachments: false,
    },
    automationRules: [],
    automationDrafts: [],
    mailboxColors: [],
    tags: [],
    smartMailboxOrder: [],
    accountOrder: [],
    mailboxGroups: [],
    compose: undoSendDelaySeconds === null ? null : { undoSendDelaySeconds },
  }
}

function scheduledResponse(sendAt: string): SendMessageResponse {
  return {
    ok: true,
    operation: {
      id: 'op-scheduled-1',
      accountId: SOURCE_ID,
      entity: { kind: 'message', id: 'send-1' },
      kind: 'send',
      payload: {},
      state: 'pending',
      attempts: 0,
      lastError: null,
      dependsOn: null,
      sendAt,
      createdAt: '2026-01-01T00:00:00Z',
      updatedAt: '2026-01-01T00:00:00Z',
    },
  }
}

function spyToast() {
  const spy = track(spyOn(sonner, 'toast').mockReturnValue('toast-1' as never))
  // The hook uses `toast.dismiss` to swap the countdown for the outcome
  // toast; the bare mock function has no methods, so restore that one.
  ;(sonner.toast as unknown as { dismiss?: (id?: unknown) => void }).dismiss =
    () => {}
  return spy
}

function undoFromToast(toastSpy: ReturnType<typeof spyOn>): () => void {
  for (let index = toastSpy.mock.calls.length - 1; index >= 0; index -= 1) {
    const options = toastSpy.mock.calls[index]?.[1] as
      | { action?: { onClick?: () => void } }
      | undefined
    if (options?.action?.onClick) {
      return options.action.onClick
    }
  }
  throw new Error('no toast registered an Undo action')
}

function renderSubmission({
  delaySeconds,
  onPersistDraft,
  onRestoreDraft,
  onSent,
}: {
  delaySeconds: number | null
  onPersistDraft?: () => Promise<void>
  onRestoreDraft?: (sourceId: string, draftKey: string) => void
  onSent?: () => void
}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  })
  // Seed the settings the hook reads the undo delay from (fresh, no fetch).
  queryClient.setQueryData(queryKeys.settings, settings(delaySeconds))
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  )
  return renderHook(
    () =>
      useComposeSubmission({
        draftKey: DRAFT_KEY,
        form: validForm(),
        intentKind: 'new',
        isPreparingMessage: false,
        onClose: () => {},
        onPersistDraft,
        onRestoreDraft,
        onSent,
        replyContext: undefined,
        resolveSubmissionSourceId: () => SOURCE_ID,
        setErrorMessage: () => {},
        setIsReadingAttachments: () => {},
      }),
    { wrapper },
  )
}

describe('useComposeSubmission — undo-send / send-later', () => {
  it('with a delay configured, Send schedules (sendAt = now + delay, draftId set) and never takes the immediate path', async () => {
    const persistCalls: string[] = []
    const scheduleSpy = track(
      spyOn(runtimeMutations.messages, 'scheduleSend').mockImplementation(
        async (request) => {
          persistCalls.push('schedule')
          return scheduledResponse(request.input.sendAt ?? '')
        },
      ),
    )
    const sendSpy = track(
      spyOn(runtimeMutations.messages, 'send').mockResolvedValue({} as never),
    )
    const toastSpy = spyToast()

    const before = Date.now()
    const { result } = renderSubmission({
      delaySeconds: 10,
      onPersistDraft: async () => {
        persistCalls.push('persist')
      },
    })
    await act(async () => {
      result.current.handleSubmit()
    })
    await waitFor(() => expect(scheduleSpy).toHaveBeenCalledTimes(1))

    expect(sendSpy).not.toHaveBeenCalled()
    const request = scheduleSpy.mock.calls[0]?.[0]
    expect(request?.sourceId).toBe(SOURCE_ID)
    expect(request?.input.draftId).toBe(DRAFT_KEY)
    const sendAtMs = new Date(request?.input.sendAt ?? '').getTime()
    expect(sendAtMs).toBeGreaterThanOrEqual(before + 10_000)
    expect(sendAtMs).toBeLessThanOrEqual(Date.now() + 11_000)
    // The draft is persisted BEFORE the schedule so undo restores fidelity.
    expect(persistCalls).toEqual(['persist', 'schedule'])
    // The countdown toast is up with an Undo action.
    await waitFor(() =>
      expect(
        toastSpy.mock.calls.some((call) =>
          String(call[0]).includes('Sending in 10s'),
        ),
      ).toBe(true),
    )
  })

  it('Undo within the window discards the queued op and reopens the draft', async () => {
    const scheduleSpy = track(
      spyOn(runtimeMutations.messages, 'scheduleSend').mockImplementation(
        async (request) => scheduledResponse(request.input.sendAt ?? ''),
      ),
    )
    const discardSpy = track(
      spyOn(runtimeMutations.messages, 'discardOperation').mockResolvedValue(
        undefined as never,
      ),
    )
    const toastSpy = spyToast()
    const restored: Array<[string, string]> = []

    const { result } = renderSubmission({
      delaySeconds: 10,
      onRestoreDraft: (sourceId, draftKey) => {
        restored.push([sourceId, draftKey])
      },
    })
    await act(async () => {
      result.current.handleSubmit()
    })
    await waitFor(() => expect(scheduleSpy).toHaveBeenCalledTimes(1))

    await act(async () => {
      undoFromToast(toastSpy)()
    })
    await waitFor(() => expect(discardSpy).toHaveBeenCalledTimes(1))
    expect(discardSpy).toHaveBeenCalledWith(SOURCE_ID, 'op-scheduled-1')
    await waitFor(() => expect(restored).toEqual([[SOURCE_ID, DRAFT_KEY]]))
  })

  it('a lost cancel race (discard rejects) reports "too late" and does NOT reopen the draft', async () => {
    track(
      spyOn(runtimeMutations.messages, 'scheduleSend').mockImplementation(
        async (request) => scheduledResponse(request.input.sendAt ?? ''),
      ),
    )
    const discardSpy = track(
      spyOn(runtimeMutations.messages, 'discardOperation').mockRejectedValue(
        new Error('operation not found (already completed)'),
      ),
    )
    const toastSpy = spyToast()
    const restored: Array<[string, string]> = []

    const { result } = renderSubmission({
      delaySeconds: 10,
      onRestoreDraft: (sourceId, draftKey) => {
        restored.push([sourceId, draftKey])
      },
    })
    await act(async () => {
      result.current.handleSubmit()
    })
    await waitFor(() => expect(toastSpy.mock.calls.length).toBeGreaterThan(0))

    await act(async () => {
      undoFromToast(toastSpy)()
    })
    await waitFor(() => expect(discardSpy).toHaveBeenCalledTimes(1))
    await waitFor(() =>
      expect(
        toastSpy.mock.calls.some((call) =>
          String(call[0]).includes('Too late to undo'),
        ),
      ).toBe(true),
    )
    expect(restored).toEqual([])
    expect(
      toastSpy.mock.calls.some((call) =>
        String(call[0]).includes('back in the composer'),
      ),
    ).toBe(false)
  })

  it('delay 0 keeps the pre-feature immediate path (send mutation, draft discarded on success)', async () => {
    const scheduleSpy = track(
      spyOn(runtimeMutations.messages, 'scheduleSend').mockResolvedValue(
        scheduledResponse('') as never,
      ),
    )
    const sendSpy = track(
      spyOn(runtimeMutations.messages, 'send').mockResolvedValue({} as never),
    )
    spyToast()
    let sentCalled = false

    const { result } = renderSubmission({
      delaySeconds: 0,
      onSent: () => {
        sentCalled = true
      },
    })
    await act(async () => {
      result.current.handleSubmit()
    })
    await waitFor(() => expect(sendSpy).toHaveBeenCalledTimes(1))

    expect(scheduleSpy).not.toHaveBeenCalled()
    expect(sendSpy.mock.calls[0]?.[0]?.sourceId).toBe(SOURCE_ID)
    // No sendAt and no draftId injection on the immediate path.
    expect(sendSpy.mock.calls[0]?.[0]?.input.sendAt).toBeUndefined()
    await waitFor(() => expect(sentCalled).toBe(true))
  })

  it('Send later schedules the explicit time with a "Scheduled for" toast (no countdown)', async () => {
    const scheduleSpy = track(
      spyOn(runtimeMutations.messages, 'scheduleSend').mockImplementation(
        async (request) => scheduledResponse(request.input.sendAt ?? ''),
      ),
    )
    const toastSpy = spyToast()

    const { result } = renderSubmission({ delaySeconds: 10 })
    await act(async () => {
      result.current.handleSubmitLater('2999-01-01T09:00:00.000Z')
    })
    await waitFor(() => expect(scheduleSpy).toHaveBeenCalledTimes(1))

    expect(scheduleSpy.mock.calls[0]?.[0]?.input.sendAt).toBe(
      '2999-01-01T09:00:00.000Z',
    )
    await waitFor(() =>
      expect(
        toastSpy.mock.calls.some(
          (call) =>
            String(call[0]).includes('Scheduled for') &&
            String(call[0]).includes('sends when Posthaste is open'),
        ),
      ).toBe(true),
    )
  })
})
