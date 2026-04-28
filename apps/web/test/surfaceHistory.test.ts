import { describe, expect, it } from 'bun:test'

import {
  currentSurfaceDepth,
  isSurfaceHistoryState,
  rootUrl,
  surfaceHistoryState,
  surfaceUrl,
} from '../src/surfaceHistory'

const mainLocation = {
  hash: '',
  pathname: '/mail',
  search: '?account=primary',
}

describe('surface history', () => {
  it('starts a surface stack from non-surface locations', () => {
    expect(currentSurfaceDepth(mainLocation, null)).toBe(0)
  })

  it('treats direct surface URLs as the first stack entry', () => {
    expect(
      currentSurfaceDepth(
        {
          hash: '#/surface/message?sourceId=primary',
          pathname: '/mail',
          search: '',
        },
        null,
      ),
    ).toBe(1)
  })

  it('uses explicit history depth when present', () => {
    const state = surfaceHistoryState('/surface/attachment', 2)

    expect(currentSurfaceDepth(mainLocation, state)).toBe(2)
    expect(isSurfaceHistoryState(state)).toBe(true)
  })

  it('builds hash URLs without dropping the current path or search', () => {
    expect(surfaceUrl(mainLocation, '/surface/message?messageId=one')).toBe(
      '/mail?account=primary#/surface/message?messageId=one',
    )
    expect(rootUrl(mainLocation)).toBe('/mail?account=primary')
  })
})
