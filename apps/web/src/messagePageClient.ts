import type { MessagePage } from './api/types'
import { runtimeViews } from './runtime/views'
import type {
  RuntimeMessagePageRequest,
  RuntimeMessagePageScope,
} from './runtime/types'

export type MessagePageScope = RuntimeMessagePageScope
export type MessagePageRequest = RuntimeMessagePageRequest

export interface MessagePageClient {
  fetchPage(req: MessagePageRequest): Promise<MessagePage>
}

export const messagePageClient: MessagePageClient = {
  fetchPage(req) {
    return runtimeViews.mail.messagePage(req)
  },
}
