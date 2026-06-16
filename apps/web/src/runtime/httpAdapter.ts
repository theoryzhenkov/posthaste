import { performMessageCommand } from '../api/client'

import type { RuntimeAdapter } from './types'

/**
 * Default runtime adapter during migration.
 *
 * It preserves production behavior by delegating to the existing typed HTTP
 * client while renderer code moves behind the runtime facade.
 */
export const httpRuntimeAdapter: RuntimeAdapter = {
  runMessageCommand({ command, messageId, sourceId }) {
    return performMessageCommand(messageId, command, sourceId)
  },
}
