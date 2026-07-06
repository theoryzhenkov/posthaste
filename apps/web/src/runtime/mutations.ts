import type {
  AccountOverview,
  AppSettings,
  AutomationRulePreviewInput,
  AutomationRulePreviewResponse,
  CreateAccountInput,
  CreateMailboxInput,
  CreateSmartMailboxInput,
  DeleteMailboxInput,
  Mailbox,
  MessageCommandResult,
  OkResponse,
  Operation,
  PatchMailboxInput,
  PatchSettingsInput,
  Rule,
  SaveDraftInput,
  SendMessageInput,
  SmartMailbox,
  SmartMailboxSummary,
  StartOAuthResponse,
  StartProviderOAuthInput,
  UpdateAccountInput,
  UpdateSmartMailboxInput,
  VerificationResponse,
  WritableRuleInput,
} from '../api/types'
import { getRuntimeAdapter } from './adapter'
import { runtimeLinkClient } from './linkClient'
import {
  foldOptimisticMailMutation,
  revertOptimisticMailMutation,
} from './replica/entityStoreAdapter'
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
    create(accountId: string, input: CreateMailboxInput): Promise<Mailbox[]> {
      return getRuntimeAdapter().createMailbox(accountId, input)
    },
    delete(
      accountId: string,
      mailboxId: string,
      input: DeleteMailboxInput,
    ): Promise<Mailbox[]> {
      return getRuntimeAdapter().deleteMailbox(accountId, mailboxId, input)
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
      const receipt = await runtimeLinkClient.runMutation({
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
      const receipt = await runtimeLinkClient.runMutation({
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
      const receipt = await runtimeLinkClient.runMutation({
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
      const receipt = await runtimeLinkClient.runMutation({
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
    /**
     * Send a message through the optimistic runtime-mutation path (M66) instead
     * of the old fire-and-forget REST POST. `message.send` folds a Destroy on the
     * originating draft's row (the blink): the draft disappears the instant the
     * user hits send, then settles/reverts on the runtime's terminal verdict —
     * a parked (dispatch-uncertain) send is NOT a confirmed send, so its rejected
     * settlement RETURNS the draft rather than leaving a false "Sent". The stable
     * draft key rides as `messageId` so the Destroy fold targets the right row
     * (mirrors `saveDraft`/`discardDraft`); the full send payload rides as
     * `request` (the far node's `message.send` args shape).
     */
    async send(request: {
      sourceId: string
      input: SendMessageInput
    }): Promise<MessageCommandResult> {
      const receipt = await runtimeLinkClient.runMutation({
        name: 'message.send',
        args: {
          sourceId: request.sourceId,
          // The originating draft's stable key: the Destroy fold keys on it.
          messageId: request.input.draftId ?? '',
          request: request.input,
        },
        sourceId: request.sourceId,
      })
      return confirmedMessageCommandResult(receipt)
    },
    /**
     * Save (create or update) a draft through the optimistic runtime-mutation
     * path (M65/D130) instead of the fire-and-forget REST POST. A save carries
     * NO optimistic fold — the fold vocabulary has no upsert and holds no draft
     * content — so this is not a "blink"; the win is the typed, idempotent
     * runMutation path (per-mutation dedup on redelivery) replacing the silent
     * POST, plus the reconciling `message.updated` the draft settlement emits
     * (D132). The stable draft key rides as `messageId`; the far node uses it as
     * the draft id and re-stamps `X-Posthaste-Draft-Id` (D131).
     */
    async saveDraft(request: {
      sourceId: string
      input: SaveDraftInput
    }): Promise<MessageCommandResult> {
      const receipt = await runtimeLinkClient.runMutation({
        name: 'message.saveDraft',
        args: {
          sourceId: request.sourceId,
          // The stable draft key: autosave always supplies it as `draftId`.
          messageId: request.input.draftId ?? '',
          request: request.input.message,
        },
        sourceId: request.sourceId,
      })
      return confirmedMessageCommandResult(receipt)
    },
    deleteDraft(request: {
      sourceId: string
      draftId: string
    }): Promise<Operation> {
      return getRuntimeAdapter().deleteDraft(request)
    },
    /**
     * FIX1 / D134 — the FOLD phase of a deferred discard. Applies the optimistic
     * destroy fold on the row (the instant blink) client-side under
     * `clientMutationId`, WITHOUT dispatching to the server or persisting a
     * durable record. The caller (`useEmailActions.discardDraft`) reuses the
     * SAME `clientMutationId` for the deferred {@link discardDraft} commit
     * (idempotent re-fold, no second blink) or reverts it via
     * {@link revertDiscard} on Undo. Returns the foldId, or null when no store is
     * active (the row removal then happens at commit, unchanged).
     */
    foldDiscard(request: {
      sourceId: string
      messageId: string
      draftId: string
      clientMutationId: string
    }): Promise<string | null> {
      return foldOptimisticMailMutation({
        name: 'message.deleteDraft',
        args: {
          sourceId: request.sourceId,
          messageId: request.messageId,
          draftId: request.draftId,
        },
        clientMutationId: request.clientMutationId,
        sourceId: request.sourceId,
      })
    },
    /**
     * FIX1 / D134 — revert a {@link foldDiscard} that was never committed (Undo
     * within the grace): the folded row returns, with no server round-trip.
     */
    revertDiscard(foldId: string): Promise<void> {
      return revertOptimisticMailMutation(foldId)
    },
    /**
     * Discard a draft through the optimistic runtime-mutation path (D130) —
     * unlike {@link deleteDraft} (a fire-and-forget POST) this folds an
     * optimistic destroy on the row's `messageId` (the blink), settles on the
     * runtime notification, and reverts + surfaces the error on failure. The
     * stable `draftId` (D131) rides along so the far node resolves the current
     * live Email even after a JMAP autosave rotates the id.
     *
     * FIX1 / D134: when the row was already folded by {@link foldDiscard}, the
     * caller threads that fold's `clientMutationId` here so this COMMIT re-runs
     * the same mutation (idempotent fold — no second blink) and simply adds the
     * durable record + server dispatch.
     */
    async discardDraft(
      request: {
        sourceId: string
        messageId: string
        draftId: string
        clientMutationId?: string
      },
      options?: { userInitiated?: boolean },
    ): Promise<MessageCommandResult> {
      const receipt = await runtimeLinkClient.runMutation({
        name: 'message.deleteDraft',
        args: {
          sourceId: request.sourceId,
          messageId: request.messageId,
          draftId: request.draftId,
        },
        ...(request.clientMutationId
          ? { clientMutationId: request.clientMutationId }
          : {}),
        sourceId: request.sourceId,
        ...(options?.userInitiated ? { context: { userInitiated: true } } : {}),
      })
      return confirmedMessageCommandResult(receipt)
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
  rules: {
    create(input: WritableRuleInput): Promise<Rule> {
      return getRuntimeAdapter().createRule(input)
    },
    update(id: string, input: WritableRuleInput): Promise<Rule> {
      return getRuntimeAdapter().updateRule(id, input)
    },
    delete(id: string): Promise<void> {
      return getRuntimeAdapter().deleteRule(id)
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
