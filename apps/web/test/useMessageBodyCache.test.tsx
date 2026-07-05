/**
 * `useMessageBody` mirrors the fetched body into `mailKeys.messageBody` so it
 * survives the detail pane unmounting — this is what lets the reply composer
 * seed its quote instantly. For an HTML message the text/plain body (which the
 * reply quote is built from) is warmed in the background into the same entry.
 */
import { afterEach, describe, expect, it, spyOn } from 'bun:test'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { renderHook, waitFor } from '@testing-library/react'
import type { ReactNode } from 'react'

import { useMessageBody } from '../src/hooks/useMessageBody'
import { mailKeys } from '../src/mailState'
import { runtimeResources } from '../src/runtime/resources'
import { setupDomEnvironment } from './dom-env'

setupDomEnvironment()

const SOURCE_ID = 'acct-1'
const MESSAGE_ID = 'msg-1'

function render(client: QueryClient) {
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={client}>{children}</QueryClientProvider>
  )
  return renderHook(() => useMessageBody(SOURCE_ID, MESSAGE_ID), { wrapper })
}

describe('useMessageBody — body cache for the reply seed', () => {
  afterEach(() => {})

  it('caches the html body and warms the text/plain body for the quote', async () => {
    const textSpy = spyOn(runtimeResources, 'text').mockImplementation(
      (descriptor) => {
        if (
          descriptor.kind === 'message-body' &&
          descriptor.format === 'html'
        ) {
          return Promise.resolve('<p>Here is the plan</p>')
        }
        return Promise.resolve('Here is the plan\nsecond line')
      },
    )
    const client = new QueryClient()
    try {
      const { result } = render(client)
      // Display resolves to the html body.
      await waitFor(() => expect(result.current.isLoading).toBe(false))
      expect(result.current.bodyHtml).toBe('<p>Here is the plan</p>')

      // The cache carries the html immediately and the text/plain warms in.
      await waitFor(() => {
        const cached = client.getQueryData(
          mailKeys.messageBody(SOURCE_ID, MESSAGE_ID),
        )
        expect(cached).toEqual({
          bodyHtml: '<p>Here is the plan</p>',
          bodyText: 'Here is the plan\nsecond line',
        })
      })
    } finally {
      textSpy.mockRestore()
    }
  })

  it('caches the text body for an html-less message', async () => {
    const textSpy = spyOn(runtimeResources, 'text').mockImplementation(
      (descriptor) => {
        if (
          descriptor.kind === 'message-body' &&
          descriptor.format === 'html'
        ) {
          return Promise.resolve('') // no html alternative
        }
        return Promise.resolve('Plain only')
      },
    )
    const client = new QueryClient()
    try {
      const { result } = render(client)
      await waitFor(() => expect(result.current.isLoading).toBe(false))
      expect(result.current.bodyText).toBe('Plain only')
      expect(
        client.getQueryData(mailKeys.messageBody(SOURCE_ID, MESSAGE_ID)),
      ).toEqual({ bodyHtml: null, bodyText: 'Plain only' })
    } finally {
      textSpy.mockRestore()
    }
  })
})
