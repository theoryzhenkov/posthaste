import { afterEach, describe, expect, it, spyOn } from 'bun:test'

import * as apiClient from '../src/api/client'
import type { Identity, ReplyContext, SendMessageInput } from '../src/api/types'
import {
  fetchRuntimeConversationPage,
  fetchRuntimeIdentity,
  fetchRuntimeReplyContext,
  fetchRuntimeSenderAddresses,
  resetRuntimeAdapterForTesting,
  sendRuntimeMessage,
} from '../src/runtime/adapter'

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
      expect(await fetchRuntimeIdentity('primary')).toBe(identity)
      expect(await fetchRuntimeSenderAddresses()).toEqual([])
      expect(await fetchRuntimeConversationPage({ limit: 75 })).toEqual({
        items: [],
        nextCursor: null,
      })
      expect(
        await fetchRuntimeReplyContext({
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

  it('wraps existing HTTP send by default', async () => {
    const sendSpy = spyOn(apiClient, 'sendMessage').mockResolvedValue({
      ok: true,
    })

    try {
      await expect(
        sendRuntimeMessage({ sourceId: 'primary', input: sendInput }),
      ).resolves.toEqual({ ok: true })
      expect(sendSpy).toHaveBeenCalledWith('primary', sendInput)
    } finally {
      sendSpy.mockRestore()
    }
  })
})
