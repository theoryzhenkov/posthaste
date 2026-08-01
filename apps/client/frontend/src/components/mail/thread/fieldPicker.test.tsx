/**
 * The picker's ITEMS are Radix-portalled and open on interaction, so no
 * DOM-less render reaches them. What is testable is the part that decides what
 * they will say — `fieldPickerOptions`, a pure function — plus the fact that
 * the visible trigger renders at all.
 */
import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import { fieldsForSurface } from '../fields'
import { FieldPickerButton, fieldPickerOptions } from './fieldPicker'

describe('offered set', () => {
  test('offers every field of the surface, in registry order', () => {
    const options = fieldPickerOptions(fieldsForSurface('detail'), [])
    expect(options.map((option) => option.id)).toEqual(
      fieldsForSurface('detail'),
    )
    expect(options[0]?.label).toBe('Subject')
  })

  test('marks the chosen fields, and only those', () => {
    const options = fieldPickerOptions(['from', 'cc', 'bcc'], ['cc'])
    expect(
      options.filter((option) => option.checked).map((option) => option.id),
    ).toEqual(['cc'])
  })

  test('ignores a selected id the surface does not offer', () => {
    // Stored selections outlive the registry; a stale or other-surface id must
    // not add an entry to the menu.
    const options = fieldPickerOptions(['from'], ['from', 'preview'])
    expect(options.map((option) => option.id)).toEqual(['from'])
  })

  test('locks only the field it is told to', () => {
    // The list's last remaining column: the table needs one to lay out, so it
    // shows as locked rather than as a click that silently does nothing.
    const options = fieldPickerOptions(['from', 'subject'], ['from'], 'from')
    expect(options.map((option) => option.locked)).toEqual([true, false])
  })

  test('locks nothing when no field is locked', () => {
    const options = fieldPickerOptions(['from', 'subject'], ['from'])
    expect(options.some((option) => option.locked)).toBe(false)
  })
})

describe('visible trigger', () => {
  test('renders a labelled button, so the picker is not right-click-only', () => {
    const html = renderToStaticMarkup(
      <FieldPickerButton
        label="Choose columns"
        options={fieldPickerOptions(['from'], ['from'])}
        onReset={() => {}}
        onToggle={() => {}}
      />,
    )
    expect(html).toContain('aria-label="Choose columns"')
    expect(html).toContain('data-slot="popover-trigger"')
  })
})
