/**
 * Account create/edit form with save, verify, and secret management.
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#secret-management
 */
import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "../ui/alert-dialog";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Checkbox } from "../ui/checkbox";
import { Input } from "../ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "../ui/select";
import { createAccount, updateAccount, verifyAccount } from "../../api/client";
import type {
  AccountOverview,
  VerificationResponse,
} from "../../api/types";
import { cn } from "../../lib/utils";
import { formatRelativeTime } from "../../utils/relativeTime";
import {
  buildCreateAccountPayload,
  buildUpdateAccountPayload,
  EMPTY_FORM,
  formFromAccount,
  parseAccountDriver,
  statusTone,
} from "./helpers";
import {
  Field,
  MetaStat,
  SectionHeader,
  StatusDot,
} from "./shared";
import type { EditorTarget } from "./types";

/**
 * Account editor form: create new or edit existing accounts.
 *
 * Supports the tri-state secret write mode (keep/replace/clear), post-save
 * JMAP session verification, and account-level actions (sync, enable/disable, delete).
 *
 * @spec docs/L1-api#account-crud-lifecycle
 * @spec docs/L1-api#secret-management
 */
export function AccountEditor({
  editorTarget,
  editingAccount,
  onSaved,
  onVerified,
  onCommand,
  isCommandPending,
}: {
  editorTarget: EditorTarget;
  editingAccount: AccountOverview | null;
  onSaved: (account: AccountOverview) => Promise<void>;
  onVerified: () => Promise<void>;
  onCommand: (
    action: "enable" | "disable" | "delete" | "sync",
    account: AccountOverview,
  ) => void;
  isCommandPending: boolean;
}) {
  const [form, setForm] = useState(() =>
    editingAccount ? formFromAccount(editingAccount) : EMPTY_FORM,
  );
  const [feedback, setFeedback] = useState<string | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [verification, setVerification] = useState<VerificationResponse | null>(null);

  const saveMutation = useMutation({
    mutationFn: (currentForm: typeof form) =>
      editorTarget === "new"
        ? createAccount(buildCreateAccountPayload(currentForm))
        : updateAccount(editorTarget, buildUpdateAccountPayload(currentForm)),
    onSuccess: async (account) => {
      setFeedback(`Saved ${account.name}.`);
      setErrorMessage(null);
      setVerification(null);
      await onSaved(account);
    },
    onError: (error: Error) => {
      setFeedback(null);
      setErrorMessage(error.message);
    },
  });

  const verifyMutation = useMutation({
    mutationFn: (accountId: string) => verifyAccount(accountId),
    onSuccess: async (result) => {
      setVerification(result);
      setFeedback(
        result.identityEmail
          ? `Verified ${result.identityEmail}.`
          : "Account verified.",
      );
      setErrorMessage(null);
      await onVerified();
    },
    onError: (error: Error) => {
      setVerification(null);
      setFeedback(null);
      setErrorMessage(error.message);
    },
  });

  const isEditing = editorTarget !== "new" && editingAccount !== null;

  return (
    <div className="space-y-6">
      <div className="space-y-3">
        <SectionHeader
          title={
            editorTarget === "new"
              ? "New account"
              : editingAccount?.name ?? "Account"
          }
          description={
            editorTarget === "new"
              ? "Configure transport details, then save and verify the connection."
              : "Update credentials or run account-level actions."
          }
          actions={
            isEditing && editingAccount ? (
              <AccountActions
                account={editingAccount}
                onCommand={onCommand}
                onVerify={() => verifyMutation.mutate(editingAccount.id)}
                isVerifying={verifyMutation.isPending}
                isCommandPending={isCommandPending}
              />
            ) : null
          }
        />

        {isEditing && editingAccount && (
          <AccountStatusStrip account={editingAccount} />
        )}
      </div>

      <div className="grid gap-4">
        <Field
          label="Account ID"
          value={form.id}
          disabled={editorTarget !== "new"}
          onChange={(value) => setForm((current) => ({ ...current, id: value }))}
        />

        <div className="grid gap-4 sm:grid-cols-2">
          <Field
            label="Account name"
            value={form.name}
            onChange={(value) => setForm((current) => ({ ...current, name: value }))}
          />
          <label className="grid gap-1.5 text-sm">
            <span className="text-muted-foreground">Driver</span>
            <Select
              value={form.driver}
              onValueChange={(value) =>
                setForm((current) => ({
                  ...current,
                  driver: parseAccountDriver(value, current.driver),
                }))
              }
            >
              <SelectTrigger className="h-9 w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="jmap">JMAP</SelectItem>
                <SelectItem value="mock">Mock</SelectItem>
              </SelectContent>
            </Select>
          </label>
        </div>

        <div className="grid gap-4 sm:grid-cols-2">
          <Field
            label="Base URL"
            value={form.baseUrl}
            placeholder="https://mail.example.com/jmap"
            onChange={(value) => setForm((current) => ({ ...current, baseUrl: value }))}
          />
          <Field
            label="Username"
            value={form.username}
            placeholder="you@example.com"
            onChange={(value) => setForm((current) => ({ ...current, username: value }))}
          />
        </div>

        <fieldset className="rounded-lg border border-border bg-background/40 p-3">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <legend className="text-sm font-medium">Password</legend>
              <p className="text-xs text-muted-foreground">
                {editingAccount?.transport.secret.configured
                  ? "A password is already stored securely. Keep, replace, or clear it."
                  : "Stored securely; the saved value can't be read back."}
              </p>
            </div>
            {editingAccount?.transport.secret.configured && (
              <Badge
                variant="outline"
                className="border-emerald-500/30 bg-emerald-500/10 font-mono text-[10px] uppercase tracking-wider text-emerald-700"
              >
                configured
              </Badge>
            )}
          </div>

          <div className="mt-3 flex flex-wrap gap-2">
            {(["keep", "replace", "clear"] as const).map((mode) => {
              const showKeep =
                mode !== "keep" || Boolean(editingAccount?.transport.secret.configured);
              if (!showKeep) {
                return null;
              }
              return (
                <Button
                  key={mode}
                  size="xs"
                  type="button"
                  variant={form.secretMode === mode ? "default" : "outline"}
                  onClick={() => {
                    if (
                      mode === "clear" &&
                      editingAccount?.transport.secret.configured &&
                      !window.confirm(
                        "Are you sure? The stored password will be permanently removed.",
                      )
                    ) {
                      return;
                    }
                    setForm((current) => ({
                      ...current,
                      secretMode: mode,
                      password: mode === "replace" ? current.password : "",
                    }));
                  }}
                >
                  {mode}
                </Button>
              );
            })}
          </div>

          <div className="mt-3 grid gap-1.5">
            <label className="text-sm text-muted-foreground" htmlFor="account-password">
              Password
            </label>
            <Input
              id="account-password"
              type="password"
              className="h-9"
              value={form.password}
              disabled={form.secretMode !== "replace"}
              placeholder={
                form.secretMode === "replace" ? "Enter a new password" : "Password hidden"
              }
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  password: event.target.value,
                }))
              }
            />
          </div>
        </fieldset>

        <label className="flex items-center gap-2 text-sm text-muted-foreground">
          <Checkbox
            checked={form.enabled}
            onCheckedChange={(checked) =>
              setForm((current) => ({ ...current, enabled: checked === true }))
            }
          />
          Account enabled
        </label>

        {feedback && (
          <p className="rounded border border-emerald-500/20 bg-emerald-500/5 px-3 py-2 text-sm text-emerald-700">
            {feedback}
          </p>
        )}
        {errorMessage && (
          <p className="rounded border border-destructive/20 bg-destructive/5 px-3 py-2 text-sm text-destructive">
            {errorMessage}
          </p>
        )}
        {verification && (
          <dl className="grid grid-cols-2 gap-3 rounded-lg border border-border bg-background/40 px-3 py-3 text-sm">
            <MetaStat label="Identity" value={verification.identityEmail ?? "Unknown"} />
            <MetaStat
              label="Push"
              value={verification.pushSupported ? "supported" : "unsupported"}
            />
          </dl>
        )}

        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            onClick={() => saveMutation.mutate(form)}
            disabled={saveMutation.isPending}
          >
            {editorTarget === "new" ? "Create account" : "Save changes"}
          </Button>
          <Button
            type="button"
            variant="outline"
            onClick={() =>
              setForm(editingAccount ? formFromAccount(editingAccount) : EMPTY_FORM)
            }
          >
            Reset form
          </Button>
        </div>
      </div>
    </div>
  );
}

function AccountActions({
  account,
  onCommand,
  onVerify,
  isVerifying,
  isCommandPending,
}: {
  account: AccountOverview;
  onCommand: (
    action: "enable" | "disable" | "delete" | "sync",
    account: AccountOverview,
  ) => void;
  onVerify: () => void;
  isVerifying: boolean;
  isCommandPending: boolean;
}) {
  return (
    <>
      <Button
        size="sm"
        variant="outline"
        type="button"
        onClick={onVerify}
        disabled={isVerifying}
      >
        Verify
      </Button>
      <Button
        size="sm"
        variant="outline"
        type="button"
        onClick={() => onCommand("sync", account)}
        disabled={isCommandPending}
      >
        Sync
      </Button>
      <Button
        size="sm"
        variant="outline"
        type="button"
        onClick={() =>
          onCommand(account.enabled ? "disable" : "enable", account)
        }
        disabled={isCommandPending}
      >
        {account.enabled ? "Disable" : "Enable"}
      </Button>
      <AlertDialog>
        <AlertDialogTrigger asChild>
          <Button size="sm" variant="destructive" type="button">
            Delete
          </Button>
        </AlertDialogTrigger>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete account?</AlertDialogTitle>
            <AlertDialogDescription>
              This will permanently remove &ldquo;{account.name}&rdquo; and all synced
              data. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              variant="destructive"
              onClick={() => onCommand("delete", account)}
            >
              Delete account
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}

function AccountStatusStrip({ account }: { account: AccountOverview }) {
  return (
    <>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <span className="flex items-center gap-1.5">
          <StatusDot status={account.status} />
          <span
            className={cn(
              "font-mono uppercase tracking-wider",
              statusTone(account.status).split(" ")[0],
            )}
          >
            {account.status}
          </span>
        </span>
        <span>
          Last sync:{" "}
          {account.lastSyncAt ? formatRelativeTime(account.lastSyncAt) : "never"}
        </span>
        <span>Real-time: {account.push}</span>
        <span>{account.driver.toUpperCase()}</span>
      </div>
      {account.lastSyncError && (
        <p className="rounded border border-destructive/20 bg-destructive/5 px-2 py-1.5 text-xs text-destructive">
          {account.lastSyncError}
        </p>
      )}
    </>
  );
}
