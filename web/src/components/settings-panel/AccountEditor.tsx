import { useMutation } from "@tanstack/react-query";
import { useState } from "react";
import { createAccount, updateAccount, verifyAccount } from "../../api/client";
import type {
  AccountOverview,
  VerificationResponse,
} from "../../api/types";
import {
  buildCreateAccountPayload,
  buildUpdateAccountPayload,
  EMPTY_FORM,
  formFromAccount,
  parseAccountDriver,
} from "./helpers";
import { Field, MetaStat } from "./shared";
import type { EditorTarget } from "./types";
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

export function AccountEditor({
  editorTarget,
  editingAccount,
  onSaved,
  onVerified,
}: {
  editorTarget: EditorTarget;
  editingAccount: AccountOverview | null;
  onSaved: (account: AccountOverview) => Promise<void>;
  onVerified: () => Promise<void>;
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

  return (
    <section className="rounded-xl border border-border bg-background/70 p-4">
      <p className="text-[10px] font-mono uppercase tracking-[0.24em] text-muted-foreground">
        {editorTarget === "new" ? "new account" : "account editor"}
      </p>
      <div className="mt-2 flex items-center justify-between gap-3">
        <div>
          <h3 className="text-base font-semibold tracking-tight">
            {editorTarget === "new"
              ? "Create account"
              : editingAccount?.name ?? "Edit account"}
          </h3>
          <p className="text-sm text-muted-foreground">
            Save first, then verify against the configured daemon secret store.
          </p>
        </div>
        {editorTarget !== "new" && (
          <Button
            size="sm"
            variant="outline"
            type="button"
            onClick={() => verifyMutation.mutate(editorTarget)}
          >
            Verify saved account
          </Button>
        )}
      </div>

      <div className="mt-4 grid gap-4">
        <Field
          label="Account ID"
          value={form.id}
          disabled={editorTarget !== "new"}
          onChange={(value) =>
            setForm((current) => ({ ...current, id: value }))
          }
        />

        <div className="grid grid-cols-2 gap-4">
          <Field
            label="Account name"
            value={form.name}
            onChange={(value) => setForm((current) => ({ ...current, name: value }))}
          />
          <div className="grid gap-1.5 text-sm">
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
          </div>
        </div>

        <div className="grid grid-cols-2 gap-4">
          <Field
            label="Base URL"
            value={form.baseUrl}
            placeholder="https://mail.example.com/jmap"
            onChange={(value) =>
              setForm((current) => ({ ...current, baseUrl: value }))
            }
          />
          <Field
            label="Username"
            value={form.username}
            placeholder="you@example.com"
            onChange={(value) =>
              setForm((current) => ({ ...current, username: value }))
            }
          />
        </div>

        <div className="rounded-lg border border-border bg-card/60 p-3">
          <div className="flex items-center justify-between gap-3">
            <div>
              <p className="text-sm font-medium">Secure password</p>
              <p className="text-xs text-muted-foreground">
                {editingAccount?.transport.secret.configured
                  ? "A password is already stored securely. You can keep, replace, or clear it."
                  : "Passwords are write-only and stored through the OS credential store."}
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
                  onClick={() =>
                    setForm((current) => ({
                      ...current,
                      secretMode: mode,
                      password: mode === "replace" ? current.password : "",
                    }))
                  }
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
        </div>

        <label className="flex items-center gap-2 text-sm text-muted-foreground">
          <Checkbox
            checked={form.enabled}
            onCheckedChange={(checked) =>
              setForm((current) => ({
                ...current,
                enabled: checked === true,
              }))
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
          <dl className="grid grid-cols-2 gap-3 rounded-lg border border-border bg-card/60 px-3 py-3 text-sm">
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
    </section>
  );
}
