import { describe, expect, it } from 'bun:test'

import {
  attachmentSurface,
  messageSurfaceFromSelection,
  parseSurfaceRoute,
  settingsSurface,
  surfaceRoute,
} from '../src/surfaces'

describe('surface routes', () => {
  it('round trips focused message surfaces', () => {
    const surface = messageSurfaceFromSelection({
      conversationId: 'conversation/1',
      sourceId: 'source:primary',
      messageId: 'message 1',
    })

    expect(parseSurfaceRoute(surfaceRoute(surface))).toEqual(surface)
  })

  it('round trips focused attachment surfaces', () => {
    const surface = attachmentSurface({
      sourceId: 'source:primary',
      messageId: 'message 1',
      attachmentId: 'part/2',
    })

    expect(parseSurfaceRoute(surfaceRoute(surface))).toEqual(surface)
  })

  it('round trips settings surfaces', () => {
    const surface = settingsSurface({
      category: 'accounts',
      accountId: 'primary',
      smartMailboxId: null,
    })

    expect(parseSurfaceRoute(surfaceRoute(surface))).toEqual(surface)
  })

  it('rejects incomplete message routes', () => {
    expect(parseSurfaceRoute('/surface/message?sourceId=primary')).toBeNull()
  })

  it('rejects incomplete attachment routes', () => {
    expect(
      parseSurfaceRoute('/surface/attachment?sourceId=primary&messageId=one'),
    ).toBeNull()
  })

  it('rejects unknown settings categories', () => {
    expect(parseSurfaceRoute('/surface/settings?category=advanced')).toBeNull()
  })
})
