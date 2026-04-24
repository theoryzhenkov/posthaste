/**
 * Compose and reply overlay backed by the Rust JMAP send API.
 *
 * @spec docs/L1-ui#component-hierarchy
 * @spec docs/L1-compose#mime-structure
 */
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, Mail, Reply, Send, X } from "lucide-react";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type SetStateAction,
} from "react";
import { toast } from "sonner";

import { fetchIdentity, fetchReplyContext, sendMessage } from "@/api/client";
import type { Recipient, SendMessageInput } from "@/api/types";
import { cn } from "@/lib/utils";

import { Button } from "./ui/button";
import { Input } from "./ui/input";

export type ComposeIntent =
  | { kind: "new"; sourceId: string }
  | { kind: "reply"; sourceId: string; messageId: string };

interface ComposeOverlayProps {
  intent: ComposeIntent;
  onClose: () => void;
}

interface ComposeForm {
  to: string;
  cc: string;
  bcc: string;
  subject: string;
  body: string;
}

const EMPTY_FORM: ComposeForm = {
  to: "",
  cc: "",
  bcc: "",
  subject: "",
  body: "",
};

function formatRecipient(recipient: Recipient): string {
  return recipient.name ? `${recipient.name} <${recipient.email}>` : recipient.email;
}

function formatRecipients(recipients: Recipient[]): string {
  return recipients.map(formatRecipient).join(", ");
}

function parseRecipients(value: string): Recipient[] {
  return value
    .split(/[;,]/)
    .map((part) => part.trim())
    .filter(Boolean)
    .map((part) => {
      const match = part.match(/^(.*)<([^>]+)>$/);
      if (!match) {
        return { name: null, email: part };
      }
      const name = match[1].trim().replace(/^"|"$/g, "");
      return {
        name: name || null,
        email: match[2].trim(),
      };
    });
}

function buildSendInput(form: ComposeForm): SendMessageInput {
  return {
    to: parseRecipients(form.to),
    cc: parseRecipients(form.cc),
    bcc: parseRecipients(form.bcc),
    subject: form.subject.trim(),
    body: form.body,
    inReplyTo: null,
    references: null,
  };
}

export function ComposeOverlay({ intent, onClose }: ComposeOverlayProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  const bodyRef = useRef<HTMLTextAreaElement>(null);
  const queryClient = useQueryClient();
  const identityQuery = useQuery({
    queryKey: ["identity", intent.sourceId],
    queryFn: () => fetchIdentity(intent.sourceId),
  });
  const replyContextQuery = useQuery({
    queryKey:
      intent.kind === "reply"
        ? ["reply-context", intent.sourceId, intent.messageId]
        : ["reply-context", null],
    queryFn: () =>
      fetchReplyContext(
        intent.sourceId,
        intent.kind === "reply" ? intent.messageId : "",
      ),
    enabled: intent.kind === "reply",
  });

  const composeKey =
    intent.kind === "reply" ? `${intent.sourceId}:${intent.messageId}` : intent.sourceId;

  const initialForm = useMemo<ComposeForm>(() => {
    if (intent.kind === "new") {
      return EMPTY_FORM;
    }
    if (!replyContextQuery.data) {
      return EMPTY_FORM;
    }
    const quoted = replyContextQuery.data.quotedBody
      ? `\n\n${replyContextQuery.data.quotedBody}`
      : "";
    return {
      to: formatRecipients(replyContextQuery.data.to),
      cc: "",
      bcc: "",
      subject: replyContextQuery.data.replySubject,
      body: quoted,
    };
  }, [intent.kind, replyContextQuery.data]);
  const formResetKey =
    intent.kind === "reply"
      ? `${composeKey}:${replyContextQuery.data ? "ready" : "loading"}`
      : composeKey;
  const [composeState, setComposeState] = useState(() => ({
    errorMessage: null as string | null,
    form: initialForm,
    resetKey: formResetKey,
  }));

  if (composeState.resetKey !== formResetKey) {
    setComposeState({
      errorMessage: null,
      form: initialForm,
      resetKey: formResetKey,
    });
  }

  const form =
    composeState.resetKey === formResetKey ? composeState.form : initialForm;
  const errorMessage =
    composeState.resetKey === formResetKey ? composeState.errorMessage : null;
  const setForm = useCallback((nextForm: SetStateAction<ComposeForm>) => {
    setComposeState((current) => ({
      ...current,
      form:
        typeof nextForm === "function"
          ? nextForm(current.form)
          : nextForm,
    }));
  }, []);
  const setErrorMessage = useCallback((message: string | null) => {
    setComposeState((current) => ({
      ...current,
      errorMessage: message,
    }));
  }, []);

  useEffect(() => {
    if (intent.kind === "reply" && replyContextQuery.data) {
      requestAnimationFrame(() => bodyRef.current?.focus());
    }
  }, [composeKey, intent.kind, replyContextQuery.data]);

  const sendMutation = useMutation({
    mutationFn: (input: SendMessageInput) => sendMessage(intent.sourceId, input),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["sidebar"] }),
        queryClient.invalidateQueries({ queryKey: ["conversations"] }),
      ]);
      toast("Message sent");
      onClose();
    },
    onError: (error) => {
      setErrorMessage(error.message);
    },
  });

  const isPreparingReply = intent.kind === "reply" && replyContextQuery.isLoading;
  const fromLabel = useMemo(() => {
    if (identityQuery.isError) {
      return "Sender unavailable";
    }
    const identity = identityQuery.data;
    if (!identity) {
      return "Loading sender...";
    }
    return identity.name ? `${identity.name} <${identity.email}>` : identity.email;
  }, [identityQuery.data, identityQuery.isError]);

  function setField<K extends keyof ComposeForm>(field: K, value: ComposeForm[K]) {
    setForm((current) => ({ ...current, [field]: value }));
  }

  function validate(input: SendMessageInput): string | null {
    if (input.to.length === 0) {
      return "Add at least one recipient.";
    }
    if (input.to.some((recipient) => recipient.email.trim().length === 0)) {
      return "Recipient email addresses cannot be empty.";
    }
    if (input.subject.length === 0) {
      return "Add a subject.";
    }
    if (input.body.trim().length === 0) {
      return "Write a message body.";
    }
    return null;
  }

  const handleSubmit = useCallback(() => {
    const input = buildSendInput(form);
    if (intent.kind === "reply" && replyContextQuery.data) {
      input.inReplyTo = replyContextQuery.data.inReplyTo;
      input.references = replyContextQuery.data.references;
    }
    const validationError = validate(input);
    if (validationError) {
      setErrorMessage(validationError);
      return;
    }
    setErrorMessage(null);
    sendMutation.mutate(input);
  }, [form, intent.kind, replyContextQuery.data, sendMutation, setErrorMessage]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
      if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
        event.preventDefault();
        handleSubmit();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleSubmit, onClose]);

  function handleBackdropClick(event: React.MouseEvent<HTMLDivElement>) {
    if (panelRef.current && !panelRef.current.contains(event.target as Node)) {
      onClose();
    }
  }

  return (
    <div
      className="fixed inset-0 z-[80] flex items-end justify-center bg-[rgba(6,4,12,0.42)] px-4 pb-5 backdrop-blur-[18px] backdrop-saturate-150 sm:items-center sm:pb-0"
      onMouseDown={handleBackdropClick}
    >
      <div
        ref={panelRef}
        className="flex h-[min(760px,calc(100vh-40px))] w-full max-w-[860px] flex-col overflow-hidden rounded-[12px] border border-white/10 bg-[rgba(23,22,28,0.94)] text-white shadow-[0_32px_96px_rgba(0,0,0,0.62)]"
      >
        <div className="flex h-11 shrink-0 items-center gap-2 border-b border-white/10 px-3">
          <div className="flex size-7 items-center justify-center rounded-[7px] bg-white/8 text-white/72">
            {intent.kind === "reply" ? <Reply size={15} /> : <Mail size={15} />}
          </div>
          <div className="min-w-0 flex-1">
            <div className="truncate text-sm font-semibold">
              {intent.kind === "reply" ? "Reply" : "New Message"}
            </div>
            <div className="truncate text-[11px] text-white/48">{fromLabel}</div>
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon-sm"
            className="text-white/58 hover:bg-white/8 hover:text-white"
            onClick={onClose}
            aria-label="Close compose"
          >
            <X size={15} />
          </Button>
        </div>

        <div className="grid shrink-0 gap-2 border-b border-white/10 px-4 py-3">
          <ComposeLine label="To">
            <Input
              value={form.to}
              autoFocus={intent.kind === "new"}
              onChange={(event) => setField("to", event.target.value)}
              className="h-7 border-white/10 bg-white/6 text-[13px] text-white placeholder:text-white/32 focus-visible:ring-white/16"
              placeholder="name@example.com"
            />
          </ComposeLine>
          <ComposeLine label="Cc">
            <Input
              value={form.cc}
              onChange={(event) => setField("cc", event.target.value)}
              className="h-7 border-white/10 bg-white/6 text-[13px] text-white placeholder:text-white/32 focus-visible:ring-white/16"
            />
          </ComposeLine>
          <ComposeLine label="Bcc">
            <Input
              value={form.bcc}
              onChange={(event) => setField("bcc", event.target.value)}
              className="h-7 border-white/10 bg-white/6 text-[13px] text-white placeholder:text-white/32 focus-visible:ring-white/16"
            />
          </ComposeLine>
          <ComposeLine label="Subject">
            <Input
              value={form.subject}
              onChange={(event) => setField("subject", event.target.value)}
              className="h-7 border-white/10 bg-white/6 text-[13px] text-white placeholder:text-white/32 focus-visible:ring-white/16"
              placeholder="Subject"
            />
          </ComposeLine>
        </div>

        <div className="min-h-0 flex-1 bg-[rgba(9,9,13,0.22)]">
          {isPreparingReply ? (
            <div className="flex h-full items-center justify-center gap-2 text-sm text-white/52">
              <Loader2 size={16} className="animate-spin" />
              Preparing reply...
            </div>
          ) : (
            <textarea
              ref={bodyRef}
              value={form.body}
              onChange={(event) => setField("body", event.target.value)}
              className="ph-scroll h-full w-full resize-none bg-transparent px-5 py-4 font-mono text-[13px] leading-6 text-white outline-none placeholder:text-white/32"
              placeholder="Message"
              spellCheck
            />
          )}
        </div>

        <div className="flex min-h-12 shrink-0 items-center gap-3 border-t border-white/10 px-4 py-2">
          <div
            className={cn(
              "min-w-0 flex-1 truncate text-[12px]",
              errorMessage ? "text-destructive" : "text-white/42",
            )}
          >
            {errorMessage ?? "Ready"}
          </div>
          <Button
            type="button"
            variant="outline"
            className="border-white/10 bg-white/6 text-white hover:bg-white/10"
            onClick={onClose}
          >
            Cancel
          </Button>
          <Button
            type="button"
            onClick={handleSubmit}
            disabled={sendMutation.isPending || isPreparingReply}
            className="bg-brand-coral text-white hover:bg-brand-coral/90"
          >
            {sendMutation.isPending ? (
              <Loader2 size={15} className="animate-spin" />
            ) : (
              <Send size={15} />
            )}
            Send
          </Button>
        </div>
      </div>
    </div>
  );
}

function ComposeLine({
  children,
  label,
}: {
  children: React.ReactNode;
  label: string;
}) {
  return (
    <label className="grid grid-cols-[4rem_minmax(0,1fr)] items-center gap-2">
      <span className="text-right text-[12px] font-medium text-white/48">{label}</span>
      {children}
    </label>
  );
}
