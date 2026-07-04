import { afterEach, describe, expect, it, spyOn } from 'bun:test'

import * as apiClient from '../src/api/client'
import type { Identity, ReplyContext, SendMessageInput } from '../src/api/types'
import { resetRuntimeAdapterForTesting } from '../src/runtime/adapter'
import { runtimeLinkClient } from '../src/runtime/linkClient'
import { runtimeMutations } from '../src/runtime/mutations'
import { runtimeViews } from '../src/runtime/views'
import type { RuntimeMutationReceipt } from '../src/runtime/types'

const identity: Identity = {
  id: 'primary',
  name: 'Primary',
  email: 'primary@example.com',
}

const replyContext: ReplyContext = {
  to: [],
  cc: [],
  replySubject: 'Re: Subject',
  forwardSubject: 'Fwd: Subject',
  quotedBody: null,
  inReplyTo: 'message-id',
  references: 'message-id',
}

const sendInput: SendMessageInput = {
  from: null,
  to: [{ name: null, email: 'to@example.com' }],
  cc: [],
  bcc: [],
  subject: 'Subject',
  body: 'Body',
  inReplyTo: null,
  references: null,
  attachments: [],
}

afterEach(() => {
  resetRuntimeAdapterForTesting()
})

describe('runtime compose adapter', () => {
  it('wraps existing HTTP compose reads by default', async () => {
    const identitySpy = spyOn(apiClient, 'fetchIdentity').mockResolvedValue(
      identity,
    )
    const senderSpy = spyOn(
      apiClient,
      'fetchSenderAddresses',
    ).mockResolvedValue([])
    const conversationsSpy = spyOn(
      apiClient,
      'fetchConversations',
    ).mockResolvedValue({ items: [], nextCursor: null })
    const replySpy = spyOn(apiClient, 'fetchReplyContext').mockResolvedValue(
      replyContext,
    )

    try {
      expect(await runtimeViews.compose.identity('primary')).toBe(identity)
      expect(await runtimeViews.compose.senderAddresses()).toEqual([])
      expect(
        await runtimeViews.compose.conversationPage({ limit: 75 }),
      ).toEqual({
        items: [],
        nextCursor: null,
      })
      expect(
        await runtimeViews.compose.replyContext({
          sourceId: 'primary',
          messageId: 'm1',
        }),
      ).toBe(replyContext)
      expect(identitySpy).toHaveBeenCalledWith('primary')
      expect(senderSpy).toHaveBeenCalledWith()
      expect(conversationsSpy).toHaveBeenCalledWith({ limit: 75 })
      expect(replySpy).toHaveBeenCalledWith('primary', 'm1')
    } finally {
      identitySpy.mockRestore()
      senderSpy.mockRestore()
      conversationsSpy.mockRestore()
      replySpy.mockRestore()
    }
  })

  it('routes send through the typed runMutation path (M66), not the REST POST', async () => {
    // M66: send is no longer a fire-and-forget REST POST — it forwards the
    // `message.send` MailOperation (which folds a Destroy on the originating
    // draft) through the optimistic runMutation seam. The stable draft key rides
    // as `messageId`; the full send payload as `request`.
    const receipt: RuntimeMutationReceipt = {
      runtimeMutationId: 'r-1',
      clientMutationId: 'c-1',
      name: 'message.send',
      state: 'accepted',
      error: null,
      output: { events: [] },
    }
    const restSpy = spyOn(apiClient, 'sendMessage')
    const runSpy = spyOn(runtimeLinkClient, 'runMutation').mockResolvedValue(
      receipt,
    )

    try {
      await expect(
        runtimeMutations.messages.send({
          sourceId: 'primary',
          input: { ...sendInput, draftId: 'stable-draft-1' },
        }),
      ).resolves.toEqual({ events: [] })
      // The old REST POST is never taken.
      expect(restSpy).not.toHaveBeenCalled()
      expect(runSpy).toHaveBeenCalledWith({
        name: 'message.send',
        args: {
          sourceId: 'primary',
          messageId: 'stable-draft-1',
          request: { ...sendInput, draftId: 'stable-draft-1' },
        },
        sourceId: 'primary',
      })
    } finally {
      runSpy.mockRestore()
      restSpy.mockRestore()
    }
  })
})
