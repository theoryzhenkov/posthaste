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
import type {
  RuntimeMessageCommandRequest,
  RuntimeTriggerSyncRequest,
  RuntimeTriggerSyncResult,
} from './types'

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
    command(
      request: RuntimeMessageCommandRequest,
    ): Promise<MessageCommandResult> {
      return getRuntimeAdapter().runMessageCommand(request)
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
