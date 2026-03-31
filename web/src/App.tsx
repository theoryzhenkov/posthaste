import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useCallback, useState } from "react";
import { fetchReplyData } from "./api/client";
import type { Recipient } from "./api/types";
import { ComposeModal } from "./components/ComposeModal";
import { MessageDetail } from "./components/MessageDetail";
import { MessageList } from "./components/MessageList";
import { Sidebar } from "./components/Sidebar";
import { useEmailActions } from "./hooks/useEmailActions";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
});

interface ComposeState {
  isOpen: boolean;
  mode: "new" | "reply" | "replyAll" | "forward";
  replyToEmailId?: string;
  initialTo?: Recipient[];
  initialCc?: Recipient[];
  initialSubject?: string;
  initialBody?: string;
  inReplyTo?: string | null;
  references?: string | null;
}

function MailClient() {
  const [selectedMailboxId, setSelectedMailboxId] = useState<string | null>(
    null,
  );
  const [selectedEmailId, setSelectedEmailId] = useState<string | null>(null);
  const [compose, setCompose] = useState<ComposeState | null>(null);

  const actions = useEmailActions(selectedMailboxId);

  const handleSelectMailbox = useCallback((id: string) => {
    setSelectedMailboxId(id);
    setSelectedEmailId(null);
  }, []);

  const handleSelectEmail = useCallback((id: string) => {
    setSelectedEmailId(id);
  }, []);

  const openCompose = useCallback(() => {
    setCompose({ isOpen: true, mode: "new" });
  }, []);

  const openReply = useCallback(async (emailId: string) => {
    try {
      const data = await fetchReplyData(emailId);
      setCompose({
        isOpen: true,
        mode: "reply",
        replyToEmailId: emailId,
        initialTo: data.to,
        initialCc: [],
        initialSubject: data.replySubject,
        initialBody: data.quotedBody ?? "",
        inReplyTo: data.inReplyTo,
        references: data.references,
      });
    } catch {
      setCompose({ isOpen: true, mode: "reply", replyToEmailId: emailId });
    }
  }, []);

  const openReplyAll = useCallback(async (emailId: string) => {
    try {
      const data = await fetchReplyData(emailId);
      setCompose({
        isOpen: true,
        mode: "replyAll",
        replyToEmailId: emailId,
        initialTo: data.to,
        initialCc: data.cc,
        initialSubject: data.replySubject,
        initialBody: data.quotedBody ?? "",
        inReplyTo: data.inReplyTo,
        references: data.references,
      });
    } catch {
      setCompose({ isOpen: true, mode: "replyAll", replyToEmailId: emailId });
    }
  }, []);

  const openForward = useCallback(async (emailId: string) => {
    try {
      const data = await fetchReplyData(emailId);
      setCompose({
        isOpen: true,
        mode: "forward",
        replyToEmailId: emailId,
        initialTo: [],
        initialCc: [],
        initialSubject: data.forwardSubject,
        initialBody: data.quotedBody ?? "",
        inReplyTo: null,
        references: null,
      });
    } catch {
      setCompose({ isOpen: true, mode: "forward", replyToEmailId: emailId });
    }
  }, []);

  const closeCompose = useCallback(() => {
    setCompose(null);
  }, []);

  return (
    <div className="mail-layout">
      <Sidebar
        selectedMailboxId={selectedMailboxId}
        onSelectMailbox={handleSelectMailbox}
      />
      <MessageList
        mailboxId={selectedMailboxId}
        selectedEmailId={selectedEmailId}
        onSelectEmail={handleSelectEmail}
        actions={actions}
        onCompose={openCompose}
        onReply={openReply}
        onReplyAll={openReplyAll}
        onForward={openForward}
      />
      <MessageDetail
        emailId={selectedEmailId}
        actions={actions}
        onReply={openReply}
        onReplyAll={openReplyAll}
        onForward={openForward}
      />
      {compose?.isOpen && (
        <ComposeModal
          onClose={closeCompose}
          initialTo={compose.initialTo}
          initialCc={compose.initialCc}
          initialSubject={compose.initialSubject}
          initialBody={compose.initialBody}
          inReplyTo={compose.inReplyTo}
          references={compose.references}
        />
      )}
    </div>
  );
}

export default function App() {
  return (
    <QueryClientProvider client={queryClient}>
      <MailClient />
    </QueryClientProvider>
  );
}
