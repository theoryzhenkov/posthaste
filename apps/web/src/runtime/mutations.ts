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
  Operation,
  PatchMailboxInput,
  PatchSettingsInput,
  SaveDraftInput,
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

/// Map a low-level message command to its runtime operation (`MailOperation`
/// wire shape: the operation's serde tag is the `name`, its payload the
/// `args`). Every command kind is covered — the typed vocabulary (M5) includes
/// the mailbox-membership deltas, so there is no legacy fallback path.
function namedMailOperation(request: RuntimeMessageCommandRequest): {
  name: string
  args: Record<string, unknown>
} {
  const command = request.command
  switch (command.kind) {
    case 'setKeywords':
      return {
        name: 'message.setKeywords',
        args: { command: { add: command.add, remove: command.remove } },
      }
    case 'replaceMailboxes':
      return {
        name: 'message.replaceMailboxes',
        args: { mailboxIds: command.mailboxIds },
      }
    case 'destroy':
      return { name: 'message.destroy', args: {} }
    case 'addToMailbox':
      return {
        name: 'message.addToMailbox',
        args: { mailboxId: command.mailboxId },
      }
    case 'removeFromMailbox':
      return {
        name: 'message.removeFromMailbox',
        args: { mailboxId: command.mailboxId },
      }
  }
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
      options?: { userInitiated?: boolean },
    ): Promise<MessageCommandResult> {
      // Route message commands through the runtime named-mutation pipeline
      // (Phase 5a). Post-M5 the typed vocabulary covers every command kind
      // (incl. addToMailbox/removeFromMailbox), so all commands take this path.
      const named = namedMailOperation(request)
      const receipt = await runtimeSessionClient.runMutation({
        name: named.name,
        args: {
          sourceId: request.sourceId,
          messageId: request.messageId,
          ...named.args,
        },
        clientMutationId: request.clientMutationId,
        sourceId: request.sourceId,
        // Tag user-initiated actions so the replica adapter records them as
        // undoable steps (internal/side-effect mutations — e.g. auto-mark-read —
        // omit this + don't pollute the undo history). @spec Phase 2 Slice 5d
        ...(options?.userInitiated ? { context: { userInitiated: true } } : {}),
      })
      return confirmedMessageCommandResult(receipt)
    },
    async moveToMailboxRole(
      request: RuntimeMoveMessageToMailboxRoleRequest,
      options?: { userInitiated?: boolean },
    ): Promise<MessageCommandResult> {
      const receipt = await runtimeSessionClient.runMutation({
        name: 'message.moveToRole',
        args: {
          sourceId: request.sourceId,
          messageId: request.messageId,
          role: request.role,
        },
        sourceId: request.sourceId,
        ...(options?.userInitiated ? { context: { userInitiated: true } } : {}),
      })
      return confirmedMessageCommandResult(receipt)
    },
    async snooze(
      request: { sourceId: string; messageId: string; until: number },
      options?: { userInitiated?: boolean },
    ): Promise<MessageCommandResult> {
      const receipt = await runtimeSessionClient.runMutation({
        name: 'message.snooze',
        args: {
          sourceId: request.sourceId,
          messageId: request.messageId,
          until: request.until,
        },
        sourceId: request.sourceId,
        ...(options?.userInitiated ? { context: { userInitiated: true } } : {}),
      })
      return confirmedMessageCommandResult(receipt)
    },
    async unsnooze(
      request: { sourceId: string; messageId: string },
      options?: { userInitiated?: boolean },
    ): Promise<MessageCommandResult> {
      const receipt = await runtimeSessionClient.runMutation({
        name: 'message.unsnooze',
        args: {
          sourceId: request.sourceId,
          messageId: request.messageId,
        },
        sourceId: request.sourceId,
        ...(options?.userInitiated ? { context: { userInitiated: true } } : {}),
      })
      return confirmedMessageCommandResult(receipt)
    },
    send(request: {
      sourceId: string
      input: SendMessageInput
    }): Promise<OkResponse> {
      return getRuntimeAdapter().sendMessage(request)
    },
    saveDraft(request: {
      sourceId: string
      input: SaveDraftInput
    }): Promise<Operation> {
      return getRuntimeAdapter().saveDraft(request)
    },
    deleteDraft(request: {
      sourceId: string
      draftId: string
    }): Promise<Operation> {
      return getRuntimeAdapter().deleteDraft(request)
    },
    listPendingOperations(sourceId: string): Promise<Operation[]> {
      return getRuntimeAdapter().listPendingOperations(sourceId)
    },
    discardOperation(sourceId: string, operationId: string): Promise<void> {
      return getRuntimeAdapter().discardOperation(sourceId, operationId)
    },
    retryOperation(sourceId: string, operationId: string): Promise<void> {
      return getRuntimeAdapter().retryOperation(sourceId, operationId)
    },
  },
  oauth: {
    startProvider(input: StartProviderOAuthInput): Promise<StartOAuthResponse> {
      return getRuntimeAdapter().startProviderOAuth(input)
    },
  },
  settings: {
    patch(input: PatchSettingsInput): Promise<AppSettings> {
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
