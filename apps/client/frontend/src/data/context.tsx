// React context for the MailClient facade: the one way components reach the
// API. The provider is mounted once, at the app root, around the react-query
// provider; everything below reads state through the query hooks in
// `queries.ts` and writes through the verbs in `commands.ts`.

import { createContext, useContext, type ReactNode } from 'react'
import { MailClient } from '@/client'

const MailClientContext = createContext<MailClient | null>(null)

export function MailClientProvider({
  client,
  children,
}: {
  client: MailClient
  children: ReactNode
}) {
  return (
    <MailClientContext.Provider value={client}>{children}</MailClientContext.Provider>
  )
}

export function useMailClient(): MailClient {
  const client = useContext(MailClientContext)
  if (!client) {
    throw new Error('useMailClient requires a <MailClientProvider> ancestor')
  }
  return client
}

/** The facade when mounted under a provider, `null` otherwise — for
 * components that also render in provider-less harnesses (tests). */
export function useOptionalMailClient(): MailClient | null {
  return useContext(MailClientContext)
}
