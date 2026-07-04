import { afterEach, beforeEach, describe, expect, it } from 'bun:test'
import { renderHook, act } from '@testing-library/react'

import {
  resetShowSourceMailboxForTesting,
  useShowSourceMailbox,
} from '../src/components/message-list/useShowSourceMailbox'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const STORAGE_KEY = 'posthaste-show-source-mailbox-v1'

beforeEach(() => {
  localStorage.removeItem(STORAGE_KEY)
  resetShowSourceMailboxForTesting()
})

afterEach(() => {
  localStorage.removeItem(STORAGE_KEY)
  resetShowSourceMailboxForTesting()
})

describe('useShowSourceMailbox', () => {
  it('falls back to the caller-supplied default when the view has no explicit override', () => {
    const { result: onDefault } = renderHook(() =>
      useShowSourceMailbox('smart:unified', true),
    )
    expect(onDefault.current.show).toBe(true)

    const { result: offDefault } = renderHook(() =>
      useShowSourceMailbox('source:acct:inbox', false),
    )
    expect(offDefault.current.show).toBe(false)
  })

  it('persists an explicit choice for a view, independent of its default', () => {
    const { result } = renderHook(() =>
      useShowSourceMailbox('source:acct:inbox', false),
    )
    expect(result.current.show).toBe(false)

    act(() => {
      result.current.setShow(true)
    })
    expect(result.current.show).toBe(true)

    const raw = localStorage.getItem(STORAGE_KEY)
    expect(raw).not.toBeNull()
    expect(JSON.parse(raw as string)).toEqual({ 'source:acct:inbox': true })
  })

  it('keys the override by view, so toggling one view does not affect another', () => {
    const { result: viewA } = renderHook(() =>
      useShowSourceMailbox('smart:unified', true),
    )
    const { result: viewB } = renderHook(() =>
      useShowSourceMailbox('source:acct:inbox', false),
    )

    act(() => {
      viewA.current.setShow(false)
    })
    expect(viewA.current.show).toBe(false)
    expect(viewB.current.show).toBe(false) // still its own default, untouched
  })

  it('syncs a persisted choice across concurrent instances of the same view', () => {
    const { result: instanceOne } = renderHook(() =>
      useShowSourceMailbox('smart:unified', true),
    )
    const { result: instanceTwo } = renderHook(() =>
      useShowSourceMailbox('smart:unified', true),
    )

    act(() => {
      instanceOne.current.setShow(false)
    })

    expect(instanceOne.current.show).toBe(false)
    expect(instanceTwo.current.show).toBe(false)
  })

  it('toggleShow flips the current effective value (including the default)', () => {
    const { result } = renderHook(() =>
      useShowSourceMailbox('smart:unified', true),
    )
    expect(result.current.show).toBe(true)

    act(() => {
      result.current.toggleShow()
    })
    expect(result.current.show).toBe(false)

    act(() => {
      result.current.toggleShow()
    })
    expect(result.current.show).toBe(true)
  })
})
