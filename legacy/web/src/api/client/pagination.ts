import type { OperationContext } from '../../observability'
import type { MessageSortField } from '../types'

export interface MessagePageInput {
  q?: string
  limit?: number
  cursor?: string | null
  sort?: MessageSortField
  sortDir?: string
  signal?: AbortSignal
  operation?: OperationContext
}
