import { describe, expect, test } from 'bun:test'

import { attachmentSurface, composeSurface } from '../domain/surface'
import {
  surfacePopupFeatures,
  surfaceWindowPolicy,
  surfaceWindowUrl,
} from './window'

const attachment = attachmentSurface({
  sourceId: 'src-1',
  messageId: 'msg-1',
  attachmentId: 'att-1',
})

describe('surfaceWindowPolicy', () => {
  test('every surface kind resolves a title and a popup size', () => {
    const policy = surfaceWindowPolicy(attachment)
    expect(policy.title).toBe('Attachment')
    expect(policy.popupSize.width).toBeGreaterThan(0)
    expect(policy.popupSize.height).toBeGreaterThan(0)
  })
})

describe('surfacePopupFeatures', () => {
  test('derives window.open features from the policy size', () => {
    const { width, height } = surfaceWindowPolicy(attachment).popupSize
    expect(surfacePopupFeatures(attachment)).toBe(
      `popup,width=${width},height=${height},resizable=yes,scrollbars=yes`,
    )
  })
})

describe('surfaceWindowUrl', () => {
  test('strips path and search, keeps origin, sets the surface hash route', () => {
    const location = {
      href: 'https://mail.example/app/inbox?tab=1#/old',
    } as Location
    const url = new URL(
      surfaceWindowUrl(location, composeSurface({ kind: 'new', sourceId: 'src-1' })),
    )
    expect(url.origin).toBe('https://mail.example')
    expect(url.pathname).toBe('/')
    expect(url.search).toBe('')
    expect(url.hash.startsWith('#/surface/')).toBe(true)
  })
})
