import { describe, expect, it } from 'bun:test'

import {
  accountSettingsSurface,
  attachmentSurface,
  composeSurface,
  messageSurfaceFromSelection,
  newAccountSettingsSurface,
  newSmartMailboxSettingsSurface,
  parseSurfaceRoute,
  SETTINGS_SURFACE_CATEGORIES,
  sourceMailboxSettingsSurface,
  surfaceRoute,
  surfaceRouteStateFromLocation,
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

  it('round trips compose surfaces', () => {
    expect(
      parseSurfaceRoute(
        surfaceRoute(composeSurface({ kind: 'new', sourceId: 'primary' })),
      ),
    ).toEqual(composeSurface({ kind: 'new', sourceId: 'primary' }))
    expect(
      parseSurfaceRoute(
        surfaceRoute(
          composeSurface({
            kind: 'reply',
            sourceId: 'source:primary',
            messageId: 'message 1',
          }),
        ),
      ),
    ).toEqual(
      composeSurface({
        kind: 'reply',
        sourceId: 'source:primary',
        messageId: 'message 1',
      }),
    )
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

  it('classifies valid invalid and non-surface locations', () => {
    expect(
      surfaceRouteStateFromLocation({
        hash: '#/surface/compose?composeKind=new&sourceId=primary',
        pathname: '/',
        search: '',
      }),
    ).toEqual({
      kind: 'valid',
      route: '/surface/compose?composeKind=new&sourceId=primary',
      surface: composeSurface({ kind: 'new', sourceId: 'primary' }),
    })
    expect(
      surfaceRouteStateFromLocation({
        hash: '#/surface/compose?composeKind=new',
        pathname: '/',
        search: '',
      }),
    ).toEqual({
      kind: 'invalid',
      route: '/surface/compose?composeKind=new',
    })
    expect(
      surfaceRouteStateFromLocation({
        hash: '',
        pathname: '/',
        search: '?view=inbox',
      }),
    ).toEqual({ kind: 'none' })
  })

  it('rejects incomplete message routes', () => {
    expect(parseSurfaceRoute('/surface/message?sourceId=primary')).toBeNull()
  })

  it('rejects incomplete attachment routes', () => {
    expect(
      parseSurfaceRoute('/surface/attachment?sourceId=primary&messageId=one'),
    ).toBeNull()
  })

  it('rejects unknown or duplicated route params', () => {
    expect(
      parseSurfaceRoute(
        '/surface/message?conversationId=c1&sourceId=primary&messageId=m1&draftId=d1',
      ),
    ).toBeNull()
    expect(
      parseSurfaceRoute(
        '/surface/attachment?sourceId=primary&messageId=m1&attachmentId=a1&attachmentId=a2',
      ),
    ).toBeNull()
    expect(
      parseSurfaceRoute(
        '/surface/compose?composeKind=new&sourceId=primary&draftId=d1',
      ),
    ).toBeNull()
    expect(
      parseSurfaceRoute('/surface/settings?category=accounts&draftId=d1'),
    ).toBeNull()
  })

  it('rejects incomplete compose routes', () => {
    expect(parseSurfaceRoute('/surface/compose?composeKind=new')).toBeNull()
    expect(
      parseSurfaceRoute('/surface/compose?composeKind=reply&sourceId=primary'),
    ).toBeNull()
    expect(
      parseSurfaceRoute(
        '/surface/compose?composeKind=new&sourceId=primary&messageId=one',
      ),
    ).toBeNull()
  })

  it('rejects unknown settings categories', () => {
    expect(parseSurfaceRoute('/surface/settings?category=advanced')).toBeNull()
  })

  it('accepts every declared settings category (no validator/type drift)', () => {
    // Regression: `tags` was present in the type union and the rail but absent
    // from the route validator's hand-maintained list, so
    // /surface/settings?category=tags parsed to null → "Surface route
    // unavailable". The validator now derives from SETTINGS_SURFACE_CATEGORIES.
    for (const category of SETTINGS_SURFACE_CATEGORIES) {
      const parsed = parseSurfaceRoute(`/surface/settings?category=${category}`)
      expect(parsed).not.toBeNull()
      expect(parsed?.kind).toBe('settings')
      expect(
        (parsed as { params: { category?: string } }).params.category,
      ).toBe(category)
    }
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
