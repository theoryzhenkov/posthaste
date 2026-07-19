import { describe, expect, test } from 'bun:test'

import {
  anyFieldDirty,
  clearableTextField,
  fieldPatch,
  formFieldSetter,
  mergeSparsePatch,
  textField,
} from './fields'

interface Form {
  title: string
  note: string
}

const title = textField<Form>('title', (form) => form.title)
const note = clearableTextField<Form>('note', (form) => form.note)
const saved: Form = { title: 'Inbox', note: 'keep me' }

describe('fieldPatch', () => {
  test('an untouched field emits keep', () => {
    expect(fieldPatch(note, saved, saved)).toEqual({ kind: 'keep' })
    expect(fieldPatch(title, saved, saved)).toEqual({ kind: 'keep' })
  })

  test('whitespace-only drift still counts as untouched', () => {
    const form = { ...saved, note: '  keep me ' }
    expect(fieldPatch(note, form, saved)).toEqual({ kind: 'keep' })
  })

  test('an edited clearable field sets the trimmed text', () => {
    const form = { ...saved, note: '  new note ' }
    expect(fieldPatch(note, form, saved)).toEqual({
      kind: 'set',
      value: 'new note',
    })
  })

  test('an emptied clearable field clears', () => {
    const form = { ...saved, note: '   ' }
    expect(fieldPatch(note, form, saved)).toEqual({ kind: 'clear' })
  })

  test('an emptied plain text field sets the empty trim (never clears)', () => {
    const form = { ...saved, title: ' ' }
    expect(fieldPatch(title, form, saved)).toEqual({ kind: 'set', value: '' })
  })
})

describe('anyFieldDirty', () => {
  const fields = [title, note]

  test('false when every field matches the baseline', () => {
    expect(anyFieldDirty(fields, { ...saved }, saved)).toBe(false)
  })

  test('true when any field drifted', () => {
    expect(anyFieldDirty(fields, { ...saved, title: 'Archive' }, saved)).toBe(
      true,
    )
  })
})

describe('formFieldSetter', () => {
  test('sets exactly the named field through the state updater', () => {
    let state: Form = { ...saved }
    const setField = formFieldSetter<Form>((action) => {
      state = typeof action === 'function' ? action(state) : action
    })
    setField('note')('changed')
    expect(state).toEqual({ title: 'Inbox', note: 'changed' })
  })
})

describe('mergeSparsePatch', () => {
  type Overlay = { fg?: string; bg?: string; icon?: string }
  const fields = ['fg', 'bg', 'icon'] as const
  const overlay: Overlay = { fg: '#111', bg: '#eee' }

  test('absent keys keep the saved values', () => {
    expect(mergeSparsePatch<Overlay>(fields, overlay, { icon: 'star' })).toEqual(
      { fg: '#111', bg: '#eee', icon: 'star' },
    )
  })

  test('undefined values clear their field', () => {
    expect(mergeSparsePatch<Overlay>(fields, overlay, { bg: undefined })).toEqual(
      { fg: '#111' },
    )
  })

  test('null when every field ends up cleared', () => {
    expect(
      mergeSparsePatch<Overlay>(fields, overlay, {
        fg: undefined,
        bg: undefined,
      }),
    ).toBeNull()
  })
})
