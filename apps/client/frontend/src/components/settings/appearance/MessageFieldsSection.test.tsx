/**
 * The settings home for both field pickers. Unlike the in-place pickers this
 * one is not behind a menu, so a static render sees the whole offered set —
 * which is the point of the section and what this checks.
 */
import { describe, expect, test } from 'bun:test'
import { renderToStaticMarkup } from 'react-dom/server'

import { fieldsForSurface, getMessageField } from '../../mail/fields'
import { MessageFieldsSection } from './MessageFieldsSection'

const html = renderToStaticMarkup(<MessageFieldsSection />)

describe('message fields settings', () => {
  test('configures both surfaces from one section', () => {
    expect(html).toContain('List columns')
    expect(html).toContain('Message header')
  })

  test('offers every field each surface can show', () => {
    for (const surface of ['list', 'detail'] as const) {
      for (const id of fieldsForSurface(surface)) {
        expect(html).toContain(getMessageField(id).label)
      }
    }
  })

  test('each surface can be reverted on its own', () => {
    expect(html.match(/Revert to default/g)).toHaveLength(2)
  })
})
