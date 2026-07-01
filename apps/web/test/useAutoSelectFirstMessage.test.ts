import { describe, expect, it, mock } from 'bun:test'
import { act, renderHook } from '@testing-library/react'

import type { MessageSummary } from '../src/api/types'
import { useAutoSelectFirstMessage } from '../src/components/message-list/useAutoSelectFirstMessage'

import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const msg = (id: string): MessageSummary =>
  ({ id, sourceId: 's1', conversationId: 'c1' }) as MessageSummary
const row = (m: MessageSummary) => ({ message: m })

function makeRows(ids: string[]) {
  return ids.map((id) => row(msg(id)))
}

describe('useAutoSelectFirstMessage', () => {
  it('anchors to the first row when the list becomes active with no selection', () => {
    const selectFirst = mock()
    const clearSelection = mock()
    const rows = makeRows(['m1', 'm2'])

    const { rerender, result } = renderHook(
      (props: { isListActive: boolean }) =>
        useAutoSelectFirstMessage({
          isListActive: props.isListActive,
          rows,
          selectedKey: null,
          currentViewKey: 'v1',
          selectFirst,
          clearSelection,
        }),
      { initialProps: { isListActive: false } },
    )

    expect(selectFirst).not.toHaveBeenCalled()

    act(() => rerender({ isListActive: true }))
    expect(selectFirst).toHaveBeenCalledTimes(1)
    expect(selectFirst.mock.calls[0][0]).toEqual(msg('m1'))
    expect(result.current.clearAndSkip).toBeTypeOf('function')
  })

  it('does nothing when a message is already selected', () => {
    const selectFirst = mock()
    renderHook(() =>
      useAutoSelectFirstMessage({
        isListActive: true,
        rows: makeRows(['m1']),
        selectedKey: 's1:m1',
        currentViewKey: 'v1',
        selectFirst,
        clearSelection: mock(),
      }),
    )
    expect(selectFirst).not.toHaveBeenCalled()
  })

  it('does nothing while the pane is not focused, even with rows and no selection', () => {
    const selectFirst = mock()
    renderHook(() =>
      useAutoSelectFirstMessage({
        isListActive: false,
        rows: makeRows(['m1']),
        selectedKey: null,
        currentViewKey: 'v1',
        selectFirst,
        clearSelection: mock(),
      }),
    )
    expect(selectFirst).not.toHaveBeenCalled()
  })

  it('clearAndSkip prevents re-anchoring after an explicit clear (preserves clear-to-expand)', () => {
    const selectFirst = mock()
    const clearSelection = mock()
    const rows = makeRows(['m1'])

    const { result, rerender } = renderHook(
      (props: { selectedKey: string | null }) =>
        useAutoSelectFirstMessage({
          isListActive: true,
          rows,
          selectedKey: props.selectedKey,
          currentViewKey: 'v1',
          selectFirst,
          clearSelection,
        }),
      { initialProps: { selectedKey: 's1:m1' } },
    )

    // User clicks the background to close the detail pane.
    act(() => result.current.clearAndSkip())
    expect(clearSelection).toHaveBeenCalledTimes(1)

    // Parent clears selection → selectedKey becomes null. The skip flag must
    // keep auto-select from immediately re-anchoring (which would reopen detail).
    act(() => rerender({ selectedKey: null }))
    expect(selectFirst).not.toHaveBeenCalled()
  })

  it('switching views re-arms auto-select (new context → re-anchor)', () => {
    const selectFirst = mock()
    const clearSelection = mock()
    const rows = makeRows(['m1'])

    const { result, rerender } = renderHook(
      (props: { currentViewKey: string; selectedKey: string | null }) =>
        useAutoSelectFirstMessage({
          isListActive: true,
          rows,
          selectedKey: props.selectedKey,
          currentViewKey: props.currentViewKey,
          selectFirst,
          clearSelection,
        }),
      { initialProps: { currentViewKey: 'v1', selectedKey: 's1:m1' } },
    )

    // Clear on the old view — skip flag set.
    act(() => result.current.clearAndSkip())
    act(() => rerender({ currentViewKey: 'v1', selectedKey: null }))
    expect(selectFirst).not.toHaveBeenCalled() // still skipped on the old view

    // Switch to a new view (parent clears selection for it). Skip resets → anchor.
    act(() => rerender({ currentViewKey: 'v2', selectedKey: null }))
    expect(selectFirst).toHaveBeenCalledTimes(1)
    expect(selectFirst.mock.calls[0][0]).toEqual(msg('m1'))
  })

  it('does not anchor when there are no rows', () => {
    const selectFirst = mock()
    renderHook(() =>
      useAutoSelectFirstMessage({
        isListActive: true,
        rows: [],
        selectedKey: null,
        currentViewKey: 'v1',
        selectFirst,
        clearSelection: mock(),
      }),
    )
    expect(selectFirst).not.toHaveBeenCalled()
  })
})
