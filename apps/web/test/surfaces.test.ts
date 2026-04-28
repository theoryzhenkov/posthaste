import { describe, expect, it } from 'bun:test'

import {
  accountSettingsSurface,
  attachmentSurface,
  messageSurfaceFromSelection,
  newAccountSettingsSurface,
  newSmartMailboxSettingsSurface,
  parseSurfaceRoute,
  sourceMailboxSettingsSurface,
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
    const surface = accountSettingsSurface('primary')

    expect(parseSurfaceRoute(surfaceRoute(surface))).toEqual(surface)
  })

  it('round trips settings create and source mailbox drill-ins', () => {
    expect(
      parseSurfaceRoute(surfaceRoute(newAccountSettingsSurface())),
    ).toEqual(newAccountSettingsSurface())
    expect(
      parseSurfaceRoute(surfaceRoute(newSmartMailboxSettingsSurface())),
    ).toEqual(newSmartMailboxSettingsSurface())

    const surface = sourceMailboxSettingsSurface('primary', 'inbox')

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

  it('rejects invalid settings targets', () => {
    expect(
      parseSurfaceRoute(
        '/surface/settings?category=mailboxes&targetKind=account&accountId=primary',
      ),
    ).toBeNull()
    expect(
      parseSurfaceRoute(
        '/surface/settings?category=accounts&targetKind=sourceMailbox&sourceAccountId=primary&sourceMailboxId=inbox',
      ),
    ).toBeNull()
    expect(
      parseSurfaceRoute('/surface/settings?targetKind=smartMailbox'),
    ).toBeNull()
  })

  // spec: docs/L0-testing#frontend-state-contracts
  it('rejects settings target routes with ids from another target kind', () => {
    expect(
      parseSurfaceRoute(
        '/surface/settings?targetKind=account&accountId=primary&smartMailboxId=sm-work',
      ),
    ).toBeNull()
    expect(
      parseSurfaceRoute(
        '/surface/settings?targetKind=newAccount&accountId=primary',
      ),
    ).toBeNull()
    expect(
      parseSurfaceRoute(
        '/surface/settings?targetKind=sourceMailbox&sourceAccountId=primary&sourceMailboxId=inbox&accountId=primary',
      ),
    ).toBeNull()
  })
})
