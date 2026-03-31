import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useCallback, useState } from "react";
import { DEFAULT_ACCOUNT_ID } from "./api/types";
import { MessageDetail } from "./components/MessageDetail";
import { MessageList } from "./components/MessageList";
import { Sidebar } from "./components/Sidebar";
import { useDaemonEvents } from "./hooks/useDaemonEvents";
import { useEmailActions } from "./hooks/useEmailActions";

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: 1,
    },
  },
});

function MailClient() {
  const accountId = DEFAULT_ACCOUNT_ID;
  const [selectedMailboxId, setSelectedMailboxId] = useState<string | null>(null);
  const [selectedEmailId, setSelectedEmailId] = useState<string | null>(null);

  useDaemonEvents(accountId);

  const actions = useEmailActions(accountId, selectedMailboxId);

  const handleSelectMailbox = useCallback((id: string) => {
    setSelectedMailboxId(id);
    setSelectedEmailId(null);
  }, []);

  const handleSelectEmail = useCallback((id: string) => {
    setSelectedEmailId(id);
  }, []);

  return (
    <div className="mail-app">
      <div className="mail-app__window">
        <header className="mail-app__chrome">
          <div className="mail-app__brand">
            <span className="mail-app__dot" aria-hidden="true" />
            <span className="mail-app__title">mail</span>
          </div>
          <nav className="mail-app__nav" aria-label="Mail sections">
            <span>daemon</span>
            <span>events</span>
            <span>mailboxes</span>
          </nav>
          <div className="mail-app__meta">
            <span>j/k navigate</span>
            <span>u read</span>
            <span>s star</span>
          </div>
        </header>
        <div className="mail-layout">
          <Sidebar
            accountId={accountId}
            selectedMailboxId={selectedMailboxId}
            onSelectMailbox={handleSelectMailbox}
          />
          <MessageList
            accountId={accountId}
            mailboxId={selectedMailboxId}
            selectedEmailId={selectedEmailId}
            onSelectEmail={handleSelectEmail}
            actions={actions}
          />
          <MessageDetail
            accountId={accountId}
            emailId={selectedEmailId}
            actions={actions}
          />
        </div>
      </div>
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
