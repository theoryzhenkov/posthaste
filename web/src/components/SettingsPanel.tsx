/**
 * Full-screen settings panel for account and smart mailbox administration.
 *
 * Left nav rail selects a category; detail area renders the active view.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#smart-mailbox-crud
 */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { FolderSearch, Mailbox, Settings as SettingsIcon } from "lucide-react";
import { useState } from "react";
import {
  deleteAccount,
  deleteSmartMailbox,
  disableAccount,
  enableAccount,
  fetchAccount,
  fetchSettings,
  fetchSmartMailbox,
  fetchSmartMailboxes,
  patchSettings,
  resetDefaultSmartMailboxes,
  triggerSync,
  updateSmartMailbox,
} from "../api/client";
import type {
  AccountOverview,
  SmartMailboxSummary,
} from "../api/types";
import { AccountsPane } from "./settings-panel/AccountsPane";
import { GeneralPane } from "./settings-panel/GeneralPane";
import {
  SettingsNav,
  type SettingsNavItem,
} from "./settings-panel/SettingsNav";
import { SmartMailboxesPane } from "./settings-panel/SmartMailboxesPane";
import type {
  EditorTarget,
  SmartMailboxEditorTarget,
} from "./settings-panel/types";

type SettingsCategory = "general" | "accounts" | "mailboxes";

const NAV_ITEMS: ReadonlyArray<SettingsNavItem<SettingsCategory>> = [
  { id: "general", label: "General", icon: SettingsIcon },
  { id: "accounts", label: "Accounts", icon: Mailbox },
  { id: "mailboxes", label: "Smart Mailboxes", icon: FolderSearch },
];

/** @spec docs/L1-api#account-crud-lifecycle */
interface SettingsPanelProps {
  accounts: AccountOverview[];
  activeAccountId: string | null;
  onActiveAccountChange: (accountId: string | null) => void;
}

/**
 * Settings panel shell: nav rail plus routed detail view.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#smart-mailbox-crud
 */
export function SettingsPanel({
  accounts,
  activeAccountId,
  onActiveAccountChange,
}: SettingsPanelProps) {
  const queryClient = useQueryClient();

  const [activeCategory, setActiveCategory] = useState<SettingsCategory>("accounts");
  const [editorTarget, setEditorTarget] = useState<EditorTarget>(
    accounts[0]?.id ?? "new",
  );
  const [smartMailboxEditorTarget, setSmartMailboxEditorTarget] =
    useState<SmartMailboxEditorTarget>("new");
  const [smartMailboxActionPendingKey, setSmartMailboxActionPendingKey] =
    useState<string | null>(null);
  const [smartMailboxActionError, setSmartMailboxActionError] =
    useState<string | null>(null);

  const settingsQuery = useQuery({
    queryKey: ["settings"],
    queryFn: fetchSettings,
  });
  const smartMailboxListQuery = useQuery({
    queryKey: ["smart-mailboxes"],
    queryFn: fetchSmartMailboxes,
  });

  const effectiveEditorTarget =
    editorTarget !== "new" && !accounts.some((account) => account.id === editorTarget)
      ? (accounts[0]?.id ?? "new")
      : editorTarget;
  const editorAccountId = effectiveEditorTarget === "new" ? null : effectiveEditorTarget;
  const accountQuery = useQuery({
    queryKey: ["account", editorAccountId],
    queryFn: () => fetchAccount(editorAccountId!),
    enabled: editorAccountId !== null,
  });
  const editingAccount =
    accountQuery.data ??
    accounts.find((account) => account.id === editorAccountId) ??
    null;

  const smartMailboxSummaries = smartMailboxListQuery.data ?? [];
  const effectiveSmartMailboxTarget =
    smartMailboxEditorTarget !== "new" &&
    !smartMailboxSummaries.some((mailbox) => mailbox.id === smartMailboxEditorTarget)
      ? "new"
      : smartMailboxEditorTarget;
  const editingSmartMailboxId =
    effectiveSmartMailboxTarget === "new" ? null : effectiveSmartMailboxTarget;
  const smartMailboxQuery = useQuery({
    queryKey: ["smart-mailbox", editingSmartMailboxId],
    queryFn: () => fetchSmartMailbox(editingSmartMailboxId!),
    enabled: editingSmartMailboxId !== null,
  });
  const editingSmartMailbox =
    smartMailboxQuery.data ??
    smartMailboxSummaries.find((mailbox) => mailbox.id === editingSmartMailboxId) ??
    null;

  const invalidateAccountQueries = async (accountId?: string) => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["settings"] }),
      queryClient.invalidateQueries({ queryKey: ["accounts"] }),
      accountId
        ? queryClient.invalidateQueries({ queryKey: ["account", accountId] })
        : Promise.resolve(),
    ]);
  };

  const invalidateSmartMailboxQueries = async (smartMailboxId?: string) => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["sidebar"] }),
      queryClient.invalidateQueries({ queryKey: ["messages"] }),
      queryClient.invalidateQueries({ queryKey: ["smart-mailboxes"] }),
      smartMailboxId
        ? queryClient.invalidateQueries({ queryKey: ["smart-mailbox", smartMailboxId] })
        : Promise.resolve(),
    ]);
  };

  const runSmartMailboxAction = async (
    pendingKey: string,
    action: () => Promise<void>,
  ) => {
    if (smartMailboxActionPendingKey !== null) {
      return;
    }
    setSmartMailboxActionError(null);
    setSmartMailboxActionPendingKey(pendingKey);
    try {
      await action();
    } catch (error) {
      setSmartMailboxActionError(
        error instanceof Error ? error.message : "Smart mailbox action failed.",
      );
    } finally {
      setSmartMailboxActionPendingKey(null);
    }
  };

  const handleResetSmartMailboxes = () => {
    void runSmartMailboxAction("reset-defaults", async () => {
      await resetDefaultSmartMailboxes();
      await invalidateSmartMailboxQueries();
      setSmartMailboxEditorTarget("new");
    });
  };

  const handleReorderSmartMailbox = (
    mailbox: SmartMailboxSummary,
    position: number,
  ) => {
    void runSmartMailboxAction(`reorder:${mailbox.id}`, async () => {
      await updateSmartMailbox(mailbox.id, { position });
      await invalidateSmartMailboxQueries(mailbox.id);
    });
  };

  const defaultMutation = useMutation({
    mutationFn: (accountId: string | null) =>
      patchSettings({ defaultAccountId: accountId }),
    onSuccess: async () => {
      await invalidateAccountQueries();
    },
  });

  const commandMutation = useMutation({
    mutationFn: async ({
      action,
      account,
    }: {
      action: "enable" | "disable" | "delete" | "sync";
      account: AccountOverview;
    }) => {
      switch (action) {
        case "enable":
          return enableAccount(account.id);
        case "disable":
          return disableAccount(account.id);
        case "delete":
          return deleteAccount(account.id);
        case "sync":
          return triggerSync(account.id);
      }
    },
    onSuccess: async (_result, variables) => {
      await invalidateAccountQueries(variables.account.id);
      if (variables.action === "delete") {
        const fallbackAccountId =
          accounts.find(
            (account) =>
              account.id !== variables.account.id &&
              account.enabled &&
              account.isDefault,
          )?.id ??
          accounts.find(
            (account) => account.id !== variables.account.id && account.enabled,
          )?.id ??
          null;
        if (activeAccountId === variables.account.id) {
          onActiveAccountChange(fallbackAccountId);
        }
        if (effectiveEditorTarget === variables.account.id) {
          setEditorTarget(fallbackAccountId ?? "new");
        }
      }
    },
  });

  const editorKey =
    effectiveEditorTarget === "new"
      ? "account:new"
      : `account:${effectiveEditorTarget}:${editingAccount?.updatedAt ?? "pending"}`;
  const smartMailboxEditorKey =
    effectiveSmartMailboxTarget === "new"
      ? "mailbox:new"
      : `mailbox:${effectiveSmartMailboxTarget}:${editingSmartMailbox?.updatedAt ?? "pending"}`;

  return (
    <section className="flex h-full min-h-0 flex-col bg-card text-card-foreground">
      <header className="border-b border-border px-6 py-3">
        <p className="text-[10px] font-mono uppercase tracking-[0.24em] text-muted-foreground">
          posthaste
        </p>
        <h2 className="mt-1 text-lg font-semibold tracking-tight">Settings</h2>
      </header>

      <div className="grid min-h-0 flex-1 grid-cols-[180px_minmax(0,1fr)]">
        <SettingsNav
          items={NAV_ITEMS}
          activeId={activeCategory}
          onSelect={setActiveCategory}
        />

        <div className="min-h-0 overflow-hidden">
          {activeCategory === "general" && (
            <div className="h-full min-h-0 overflow-y-auto px-6 py-6">
              <GeneralPane
                accounts={accounts}
                smartMailboxes={smartMailboxSummaries}
                defaultAccountId={settingsQuery.data?.defaultAccountId}
                onDefaultAccountChange={(accountId) =>
                  defaultMutation.mutate(accountId)
                }
                isPending={defaultMutation.isPending}
              />
            </div>
          )}

          {activeCategory === "accounts" && (
            <AccountsPane
              accounts={accounts}
              selectedAccountId={effectiveEditorTarget}
              editingAccount={editingAccount}
              editorKey={editorKey}
              onSelectAccount={(accountId) => setEditorTarget(accountId)}
              onCreateAccount={() => setEditorTarget("new")}
              onCommand={(action, account) =>
                commandMutation.mutate({ action, account })
              }
              onSaved={async (account) => {
                await invalidateAccountQueries(account.id);
                setEditorTarget(account.id);
              }}
              onVerified={() => invalidateAccountQueries(editorAccountId ?? undefined)}
              commandMutation={commandMutation}
            />
          )}

          {activeCategory === "mailboxes" && (
            <SmartMailboxesPane
              smartMailboxes={smartMailboxSummaries}
              selectedMailboxId={effectiveSmartMailboxTarget}
              editingSmartMailbox={editingSmartMailbox}
              editorKey={smartMailboxEditorKey}
              actionPendingKey={smartMailboxActionPendingKey}
              actionError={smartMailboxActionError}
              onSelectMailbox={(mailboxId) => setSmartMailboxEditorTarget(mailboxId)}
              onCreateMailbox={() => setSmartMailboxEditorTarget("new")}
              onResetDefaults={handleResetSmartMailboxes}
              onReorderMailbox={handleReorderSmartMailbox}
              onSaved={async (mailbox) => {
                await invalidateSmartMailboxQueries(mailbox.id);
                setSmartMailboxEditorTarget(mailbox.id);
              }}
              onDeleted={async (mailboxId) => {
                await deleteSmartMailbox(mailboxId);
                await invalidateSmartMailboxQueries();
                setSmartMailboxEditorTarget("new");
              }}
            />
          )}
        </div>
      </div>
    </section>
  );
}
