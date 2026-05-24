import { describe, expect, it } from 'bun:test'

import { surfaceWindowUrl } from '../src/surfaceWindow'
import { attachmentSurface } from '../src/surfaces'

describe('surface window URLs', () => {
  it('builds a root hash URL for separate surface windows', () => {
    const location = new URL(
      'https://posthaste.example/mailbox/inbox?filter=unread#old',
    ) as unknown as Location
    const surface = attachmentSurface({
      sourceId: 'source:primary',
      messageId: 'message 1',
      attachmentId: 'part/2',
    })

    expect(surfaceWindowUrl(location, surface)).toBe(
      'https://posthaste.example/#/surface/attachment?sourceId=source%3Aprimary&messageId=message+1&attachmentId=part%2F2',
    )
  })
})
