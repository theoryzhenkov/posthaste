// The react-query client IS the mirror: every rendered fact is a /query
// answer cached under a flat family key, and the ONE invalidation policy is
// "generation advanced → invalidate everything" (see stream.ts). Answers are
// therefore fresh until the stream says otherwise — staleTime is infinite,
// and window focus / reconnect refetching is disabled because liveness is the
// stream's job, not the browser's.

import { MutationCache, QueryCache, QueryClient } from '@tanstack/react-query'

import { notifyFromError } from '@/data/notifications/notifyFromError'

export const queryClient = new QueryClient({
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
