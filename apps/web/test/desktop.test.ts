import { describe, expect, it } from 'bun:test'

import { surfacePopupFeatures } from '../src/desktop'
import { surfaceWindowUrl } from '../src/surfaceWindow'
import {
  attachmentSurface,
  messageSurfaceFromSelection,
  settingsSurface,
} from '../src/surfaces'

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

  it('uses kind-specific popup features for browser fallback windows', () => {
    expect(surfacePopupFeatures(settingsSurface())).toBe(
      'popup,width=980,height=720,resizable=yes,scrollbars=yes',
    )
    expect(
      surfacePopupFeatures(
        messageSurfaceFromSelection({
          conversationId: 'conversation-1',
          sourceId: 'primary',
          messageId: 'message-1',
        }),
      ),
    ).toBe('popup,width=900,height=760,resizable=yes,scrollbars=yes')
    expect(
      surfacePopupFeatures(
        attachmentSurface({
          sourceId: 'primary',
          messageId: 'message-1',
          attachmentId: 'part-1',
        }),
      ),
    ).toBe('popup,width=1100,height=820,resizable=yes,scrollbars=yes')
  })
})
