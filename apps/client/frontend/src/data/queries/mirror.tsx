// The react-query client IS the mirror: every rendered fact is a /query
// answer cached under a flat family key, and the ONE invalidation policy is
// "generation advanced → invalidate everything" (see transport/stream.ts).
// Answers are therefore fresh until the stream says otherwise — staleTime is
// infinite, and window focus / reconnect refetching is disabled because
// liveness is the stream's job, not the browser's.
//
// Which makes the subscription load-bearing rather than optional: a mirror
// nobody invalidates is fetched once at mount and never again. So this module
// exports the PROVIDER and not the client. Creating the mirror and subscribing
// it to the stream are one act — a window cannot obtain a QueryClient without
// also getting the thing that keeps it live, and "forgot to mount the bridge"
// stops being expressible.
//
// The mirror is window-local. Each webview is its own JS realm, so every
// window builds its own; there is no shared cache for a second subscription to
// disturb.

import {
  MutationCache,
  QueryCache,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query'
import { useState, type ReactNode } from 'react'

import { notifyFromError } from '@/data/notifications/notifyFromError'

import { useStreamInvalidation } from '../transport/stream'

function createMirror(): QueryClient {
  return new QueryClient({
    // Surface query/mutation failures in the notification center so the user
    // never has to dig through a sub-page to discover an error.
    queryCache: new QueryCache({
      onError: (error) => notifyFromError(error),
    }),
    mutationCache: new MutationCache({
      onError: (error) => notifyFromError(error),
    }),
    defaultOptions: {
      queries: {
        staleTime: Infinity,
        retry: 1,
        refetchOnWindowFocus: false,
        refetchOnReconnect: false,
      },
    },
  })
}

/** The stream subscription, as a child so it reads the mirror from context —
 *  the same client every consumer below it will read. */
function StreamInvalidation(): null {
  useStreamInvalidation()
  return null
}

/**
 * This window's mirror, live. Mount inside `MailClientProvider` (the
 * subscription rides the facade's event stream) and above everything that
 * reads a query.
 */
export function MirrorProvider({ children }: { children: ReactNode }) {
  // Created once per mount, never per render: the cache IS the state, and a
  // client rebuilt mid-life would throw the window's answers away.
  const [mirror] = useState(createMirror)
  return (
    <QueryClientProvider client={mirror}>
      <StreamInvalidation />
      {children}
    </QueryClientProvider>
  )
}
