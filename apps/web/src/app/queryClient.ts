import { QueryClient } from '@tanstack/react-query'

/** @spec docs/L1-ui#data-fetching */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
})
