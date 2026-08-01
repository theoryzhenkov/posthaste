/**
 * The settings home for both field pickers. Unlike the in-place pickers this
 * one is not behind a menu, so a static render sees the whole offered set —
 * which is the point of the section and what this checks. The emphasis
 * dropdown's OPTIONS are Radix-portalled and open on interaction, so only its
 * trigger is reachable here.
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

  test('every shown header row can be moved, and the ends cannot overshoot', () => {
    // Reordering lives here rather than on the reading pane: that pane is
    // content, and dragging content is a different promise.
    expect(html).toContain('aria-label="Move Subject up"')
    expect(html).toContain('aria-label="Move Subject down"')
    // Subject is first by default, so its "up" is the disabled one.
    expect(html).toMatch(/aria-label="Move Subject up"[^>]*disabled/)
  })

  test('a shown row can change its emphasis and drop its label', () => {
    expect(html).toContain('aria-label="Subject emphasis"')
    expect(html.match(/>Label</g)?.length ?? 0).toBeGreaterThan(0)
  })
})
