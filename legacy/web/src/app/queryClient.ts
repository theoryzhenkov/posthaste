import { MutationCache, QueryCache, QueryClient } from '@tanstack/react-query'

import { notifyFromError } from '@/notifications/notifyFromError'

/** @spec docs/L1-ui#data-fetching */
export const queryClient = new QueryClient({
  // Surface query/mutation failures in the notification center so the user
  // never has to dig through a sub-page to discover an error. Notable codes
  // (e.g. database corruption) get dedicated handling in `notifyFromError`.
  queryCache: new QueryCache({
    onError: (error) => notifyFromError(error),
  }),
  mutationCache: new MutationCache({
    onError: (error) => notifyFromError(error),
  }),
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
})
