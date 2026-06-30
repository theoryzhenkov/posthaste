import { describe, expect, it } from 'bun:test'

import {
  moveRovingKey,
  sidebarSelectionKey,
} from '../src/components/sidebar/roving'

const KEYS = ['smart:a', 'smart:b', 'tag:x', 'src:s1:inbox', 'src:s1:archive']

describe('sidebarSelectionKey', () => {
  it('keys smart and source views, ignoring tags/null', () => {
    expect(
      sidebarSelectionKey({ kind: 'smart-mailbox', id: 'a', name: 'A' }),
    ).toBe('smart:a')
    expect(
      sidebarSelectionKey({
        kind: 'source-mailbox',
        sourceId: 's1',
        mailboxId: 'inbox',
        name: 'S / Inbox',
      }),
    ).toBe('src:s1:inbox')
    expect(sidebarSelectionKey(null)).toBeNull()
  })
})

describe('moveRovingKey', () => {
  it('steps down and up between adjacent rows', () => {
    expect(moveRovingKey(KEYS, 'smart:b', 1)).toBe('tag:x')
    expect(moveRovingKey(KEYS, 'smart:b', -1)).toBe('smart:a')
  })

  it('clamps at both ends without wrapping', () => {
    expect(moveRovingKey(KEYS, 'src:s1:archive', 1)).toBe('src:s1:archive')
    expect(moveRovingKey(KEYS, 'smart:a', -1)).toBe('smart:a')
  })

  it('starts at the first/last row when nothing is focused', () => {
    expect(moveRovingKey(KEYS, null, 1)).toBe('smart:a')
    expect(moveRovingKey(KEYS, null, -1)).toBe('src:s1:archive')
  })

  it('starts from the top when the focused row vanished (e.g. collapsed)', () => {
    expect(moveRovingKey(KEYS, 'src:gone:0', 1)).toBe('smart:a')
  })

  it('returns null for an empty list', () => {
    expect(moveRovingKey([], 'smart:a', 1)).toBeNull()
  })
})
