import type {
  AccountOverview,
  AppSettings,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
  CreateAccountInput,
  CreateSmartMailboxInput,
  Mailbox,
  MessageCommandResult,
  OkResponse,
  PatchMailboxInput,
  SendMessageInput,
  SmartMailbox,
  SmartMailboxSummary,
  StartOAuthResponse,
  StartProviderOAuthInput,
  UpdateAccountInput,
  UpdateSmartMailboxInput,
  VerificationResponse,
} from '../api/types'
import { getRuntimeAdapter } from './adapter'
import { runtimeSessionClient } from './sessionClient'
import type {
  RuntimeMessageCommandRequest,
  RuntimeMoveMessageToMailboxRoleRequest,
  RuntimeMutationReceipt,
  RuntimeTriggerSyncRequest,
  RuntimeTriggerSyncResult,
} from './types'

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function isMessageCommandResult(value: unknown): value is MessageCommandResult {
  return isObject(value) && Array.isArray(value.events)
}

function confirmedMessageCommandResult(
  receipt: RuntimeMutationReceipt,
): MessageCommandResult {
  if (receipt.state === 'failed') {
    throw new Error(receipt.error?.message ?? `${receipt.name} failed`)
  }
  if (!isMessageCommandResult(receipt.output)) {
    throw new Error(`${receipt.name} did not return a message command result`)
  }
  return receipt.output
}

export const runtimeMutations = {
  accounts: {
    create(input: CreateAccountInput): Promise<AccountOverview> {
      return getRuntimeAdapter().createAccount(input)
    },
    delete(accountId: string): Promise<OkResponse> {
      return getRuntimeAdapter().deleteAccount(accountId)
    },
    disable(accountId: string): Promise<OkResponse> {
      return getRuntimeAdapter().disableAccount(accountId)
    },
    enable(accountId: string): Promise<OkResponse> {
      return getRuntimeAdapter().enableAccount(accountId)
    },
    sync(
      request: RuntimeTriggerSyncRequest,
    ): Promise<RuntimeTriggerSyncResult> {
      return getRuntimeAdapter().triggerSync(request)
    },
    update(
      accountId: string,
      input: UpdateAccountInput,
    ): Promise<AccountOverview> {
      return getRuntimeAdapter().updateAccount(accountId, input)
    },
    uploadLogo(accountId: string, file: File): Promise<AccountOverview> {
      return getRuntimeAdapter().uploadAccountLogo(accountId, file)
    },
    verify(accountId: string): Promise<VerificationResponse> {
      return getRuntimeAdapter().verifyAccount(accountId)
    },
  },
  mailboxes: {
    patch(
      accountId: string,
      mailboxId: string,
      input: PatchMailboxInput,
    ): Promise<Mailbox[]> {
      return getRuntimeAdapter().patchMailbox(accountId, mailboxId, input)
    },
  },
  messages: {
    async command(
      request: RuntimeMessageCommandRequest,
    ): Promise<MessageCommandResult> {
      if (request.command.kind !== 'setKeywords') {
        return getRuntimeAdapter().runMessageCommand(request)
      }
      const receipt = await runtimeSessionClient.runMutation({
        name: 'message.setKeywords',
        args: {
          sourceId: request.sourceId,
          messageId: request.messageId,
          command: {
            add: request.command.add,
            remove: request.command.remove,
          },
        },
        clientMutationId: request.clientMutationId,
        sourceId: request.sourceId,
      })
      return confirmedMessageCommandResult(receipt)
    },
    moveToMailboxRole(
      request: RuntimeMoveMessageToMailboxRoleRequest,
    ): Promise<MessageCommandResult> {
      return getRuntimeAdapter().moveMessageToMailboxRole(request)
    },
    send(request: {
      sourceId: string
      input: SendMessageInput
    }): Promise<OkResponse> {
      return getRuntimeAdapter().sendMessage(request)
    },
  },
  oauth: {
    startProvider(input: StartProviderOAuthInput): Promise<StartOAuthResponse> {
      return getRuntimeAdapter().startProviderOAuth(input)
    },
  },
  settings: {
    patch(input: Partial<AppSettings>): Promise<AppSettings> {
      return getRuntimeAdapter().patchSettings(input)
    },
    previewAutomationRule(
      input: AutomationRulePreviewInput,
    ): Promise<AutomationRulePreviewResponse> {
      return getRuntimeAdapter().previewAutomationRule(input)
    },
  },
  smartMailboxes: {
    create(input: CreateSmartMailboxInput): Promise<SmartMailbox> {
      return getRuntimeAdapter().createSmartMailbox(input)
    },
    delete(smartMailboxId: string): Promise<OkResponse> {
      return getRuntimeAdapter().deleteSmartMailbox(smartMailboxId)
    },
    resetDefaults(): Promise<SmartMailboxSummary[]> {
      return getRuntimeAdapter().resetDefaultSmartMailboxes()
    },
    update(
      smartMailboxId: string,
      input: UpdateSmartMailboxInput,
    ): Promise<SmartMailbox> {
      return getRuntimeAdapter().updateSmartMailbox(smartMailboxId, input)
    },
  },
}
