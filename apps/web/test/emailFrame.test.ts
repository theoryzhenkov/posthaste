import { describe, expect, it } from 'bun:test'

import { externalEmailLinkUrl } from '../src/emailLinks'

describe('email frame link handling', () => {
  it('opens only web links externally', () => {
    expect(externalEmailLinkUrl('https://example.com/path?q=1')).toBe(
      'https://example.com/path?q=1',
    )
    expect(externalEmailLinkUrl('http://example.com/')).toBe(
      'http://example.com/',
    )
    expect(externalEmailLinkUrl('mailto:hello@example.com')).toBeNull()
    expect(externalEmailLinkUrl('/settings')).toBeNull()
    expect(externalEmailLinkUrl('not a url')).toBeNull()
    expect(externalEmailLinkUrl(null)).toBeNull()
  })
})
