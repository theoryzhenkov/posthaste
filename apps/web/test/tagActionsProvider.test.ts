import { describe, expect, it } from 'bun:test'

import { createRankingContext } from '../src/components/command-palette/model'
import { createTagActionProvider } from '../src/command-search/providers/tagActions'
import type { ProviderSearchRequest } from '../src/command-search/types'

function request(
  query: string,
  hasSelectedMessage: boolean,
): ProviderSearchRequest {
  const context = createRankingContext({ hasSelectedMessage })
  return { query, limit: 20, context }
}

describe('createTagActionProvider', () => {
  it('emits nothing without a selected message', async () => {
    const provider = createTagActionProvider()
    const page = await provider.search(request('', false))
    expect(page.candidates).toHaveLength(0)
  })

  it('offers a single "Tag" command that opens the tag editor when a message is selected', async () => {
    const provider = createTagActionProvider()
    const page = await provider.search(request('', true))
    expect(page.candidates).toHaveLength(1)
    expect(page.candidates[0]?.entry.label).toBe('Tag')
    expect(page.candidates[0]?.entry.action).toEqual({
      kind: 'open-tag-editor',
    })
  })

  it('matches the "Tag" command against a typed query', async () => {
    const provider = createTagActionProvider()
    const page = await provider.search(request('tag', true))
    expect(page.candidates).toHaveLength(1)

    const noMatch = await provider.search(request('zzz-no-match', true))
    expect(noMatch.candidates).toHaveLength(0)
  })
})
