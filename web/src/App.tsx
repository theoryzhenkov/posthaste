import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useCallback, useState } from "react";
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

function MailClient() {
  const [selectedMailboxId, setSelectedMailboxId] = useState<string | null>(
    null,
  );
  const [selectedEmailId, setSelectedEmailId] = useState<string | null>(null);

  const actions = useEmailActions(selectedMailboxId);

  const handleSelectMailbox = useCallback((id: string) => {
    setSelectedMailboxId(id);
    setSelectedEmailId(null);
  }, []);

  const handleSelectEmail = useCallback((id: string) => {
    setSelectedEmailId(id);
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
      />
      <MessageDetail emailId={selectedEmailId} actions={actions} />
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
