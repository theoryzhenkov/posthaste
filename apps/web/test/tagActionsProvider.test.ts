import { describe, expect, it } from 'bun:test'

import { createRankingContext } from '../src/components/command-palette/model'
import { createTagActionProvider } from '../src/command-search/providers/tagActions'
import type {
  PaletteAction,
  ProviderSearchRequest,
} from '../src/command-search/types'
import type { TagSummary } from '../src/api/types'

const KNOWN_TAGS: TagSummary[] = [
  { name: 'work', unreadMessages: 0, totalMessages: 3 },
  { name: 'travel', unreadMessages: 0, totalMessages: 1 },
]

function request(
  query: string,
  hasSelectedMessage: boolean,
): ProviderSearchRequest {
  const context = createRankingContext({ hasSelectedMessage })
  return { query, limit: 20, context }
}

function actions(
  provider: ReturnType<typeof createTagActionProvider>,
  req: ProviderSearchRequest,
): Promise<PaletteAction[]> {
  return provider
    .search(req)
    .then((page) => page.candidates.map((candidate) => candidate.entry.action))
}

describe('createTagActionProvider', () => {
  it('emits nothing without a selected message', async () => {
    const provider = createTagActionProvider({
      readModels: { tags: KNOWN_TAGS },
      selectedMessageTags: [],
    })
    const page = await provider.search(request('', false))
    expect(page.candidates).toHaveLength(0)
  })

  it('offers "Tag message with …" over known tags not already applied', async () => {
    const provider = createTagActionProvider({
      readModels: { tags: KNOWN_TAGS },
      selectedMessageTags: ['work'],
    })
    const emitted = await actions(provider, request('', true))
    // work is already applied → only travel is offered to add.
    expect(emitted).toContainEqual({
      kind: 'add-tag-to-message',
      tag: 'travel',
    })
    expect(emitted).not.toContainEqual({
      kind: 'add-tag-to-message',
      tag: 'work',
    })
  })

  it('offers "Remove tag …" only for the selection\'s current tags', async () => {
    const provider = createTagActionProvider({
      readModels: { tags: KNOWN_TAGS },
      selectedMessageTags: ['work'],
    })
    const emitted = await actions(provider, request('', true))
    expect(emitted).toContainEqual({
      kind: 'remove-tag-from-message',
      tag: 'work',
    })
    // travel is not on the message, so it is never offered for removal.
    expect(emitted).not.toContainEqual({
      kind: 'remove-tag-from-message',
      tag: 'travel',
    })
  })

  it('offers a create-new add for a typed name that is not a known tag', async () => {
    const provider = createTagActionProvider({
      readModels: { tags: KNOWN_TAGS },
      selectedMessageTags: [],
    })
    const page = await provider.search(request('urgent', true))
    const created = page.candidates.find(
      (candidate) => candidate.entry.id === 'create:urgent',
    )
    expect(created?.entry.action).toEqual({
      kind: 'add-tag-to-message',
      tag: 'urgent',
    })
    expect(created?.entry.label).toContain('new tag')
  })

  it('does not offer create-new when the typed name is already a known tag', async () => {
    const provider = createTagActionProvider({
      readModels: { tags: KNOWN_TAGS },
      selectedMessageTags: [],
    })
    const page = await provider.search(request('work', true))
    expect(
      page.candidates.some((candidate) =>
        candidate.entry.id.startsWith('create:'),
      ),
    ).toBe(false)
  })
})
