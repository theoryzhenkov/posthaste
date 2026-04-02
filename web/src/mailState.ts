import type {
  InfiniteData,
  QueryClient,
  QueryKey,
} from "@tanstack/react-query";
import type {
  ConversationPage,
  ConversationSummary,
  ConversationView,
  DomainEvent,
  MessageDetail,
  MessageSummary,
  SourceMessageRef,
} from "./api/types";

export type MailSelection = SourceMessageRef & { conversationId: string };

export type MailViewSelection =
  | { kind: "smart-mailbox"; id: string }
  | { kind: "source-mailbox"; sourceId: string; mailboxId: string }
  | null;

export type ConversationPageSlice = {
  itemIds: string[];
  nextCursor: string | null;
};

export const mailKeys = {
  message: (sourceId: string, messageId: string) =>
    ["message", sourceId, messageId] as const,
  conversation: (conversationId: string) =>
    ["conversation", conversationId] as const,
  conversationSummary: (conversationId: string) =>
    ["conversation-summary", conversationId] as const,
  view: (selection: MailViewSelection) => {
    if (!selection) {
      return ["conversations", "none"] as const;
    }
    if (selection.kind === "smart-mailbox") {
      return ["conversations", "smart-mailbox", selection.id] as const;
    }
    return [
      "conversations",
      "source-mailbox",
      selection.sourceId,
      selection.mailboxId,
    ] as const;
  },
};

export type QuerySnapshot = {
  data: unknown;
  existed: boolean;
  queryKey: QueryKey;
};

export type KeywordState = Pick<MessageSummary, "isFlagged" | "isRead" | "keywords">;

export type KeywordPatch = {
  next: KeywordState;
  previous: KeywordState;
};

type ReconcileOptions = {
  allowHeuristicFlagClear?: boolean;
};

export type CachePatchResult = {
  incomplete: boolean;
  snapshots: QuerySnapshot[];
};

const LOCAL_MUTATION_TTL_MS = 5_000;
const localMutationEvents = new Map<string, number>();

export function deriveKeywordState(keywords: string[]): KeywordState {
  return {
    isFlagged: keywords.includes("$flagged"),
    isRead: keywords.includes("$seen"),
    keywords,
  };
}

export function normalizeConversationPage(
  queryClient: QueryClient,
  page: ConversationPage,
): ConversationPageSlice {
  upsertConversationSummaries(queryClient, page.items);
  return {
    itemIds: page.items.map((item) => item.id),
    nextCursor: page.nextCursor,
  };
}

export function upsertConversationSummaries(
  queryClient: QueryClient,
  conversations: ConversationSummary[],
) {
  for (const conversation of conversations) {
    queryClient.setQueryData(
      mailKeys.conversationSummary(conversation.id),
      conversation,
    );
  }
}

export function getConversationSummary(
  queryClient: QueryClient,
  conversationId: string,
): ConversationSummary | undefined {
  return queryClient.getQueryData<ConversationSummary>(
    mailKeys.conversationSummary(conversationId),
  );
}

function buildLocalMutationKey(
  event: Pick<DomainEvent, "accountId" | "messageId" | "topic">,
) {
  return `${event.accountId}:${event.messageId ?? "none"}:${event.topic}`;
}

function cleanupLocalMutationEvents(now: number) {
  for (const [key, expiresAt] of localMutationEvents) {
    if (expiresAt <= now) {
      localMutationEvents.delete(key);
    }
  }
}

export function recordLocalMutationEvents(events: DomainEvent[]) {
  const now = Date.now();
  cleanupLocalMutationEvents(now);
  for (const event of events) {
    if (!event.messageId) {
      continue;
    }
    localMutationEvents.set(buildLocalMutationKey(event), now + LOCAL_MUTATION_TTL_MS);
  }
}

export function shouldSuppressLocalEcho(event: DomainEvent): boolean {
  if (!event.messageId) {
    return false;
  }

  const now = Date.now();
  cleanupLocalMutationEvents(now);
  const key = buildLocalMutationKey(event);
  const expiresAt = localMutationEvents.get(key);
  if (!expiresAt || expiresAt <= now) {
    localMutationEvents.delete(key);
    return false;
  }
  localMutationEvents.delete(key);
  return true;
}

function snapshotQuery(queryClient: QueryClient, queryKey: QueryKey): QuerySnapshot {
  const state = queryClient.getQueryState(queryKey);
  return {
    data: queryClient.getQueryData(queryKey),
    existed: state !== undefined,
    queryKey,
  };
}

export function restoreSnapshots(
  queryClient: QueryClient,
  snapshots: QuerySnapshot[],
) {
  for (const snapshot of snapshots) {
    if (snapshot.existed) {
      queryClient.setQueryData(snapshot.queryKey, snapshot.data);
      continue;
    }
    queryClient.removeQueries({ queryKey: snapshot.queryKey, exact: true });
  }
}

function replaceMessageKeywords<T extends MessageSummary | MessageDetail>(
  message: T,
  keywordState: KeywordState,
): T {
  return {
    ...message,
    isFlagged: keywordState.isFlagged,
    isRead: keywordState.isRead,
    keywords: keywordState.keywords,
  };
}

function applyHeuristicConversationPatch(
  conversation: ConversationSummary,
  patch: KeywordPatch,
  options?: ReconcileOptions,
): { conversation: ConversationSummary; incomplete: boolean } {
  let incomplete = false;
  let nextConversation = conversation;

  if (patch.previous.isRead !== patch.next.isRead) {
    const unreadDelta = patch.next.isRead ? -1 : 1;
    nextConversation = {
      ...nextConversation,
      unreadCount: Math.max(0, nextConversation.unreadCount + unreadDelta),
    };
  }

  if (patch.previous.isFlagged !== patch.next.isFlagged) {
    if (patch.next.isFlagged) {
      nextConversation = { ...nextConversation, isFlagged: true };
    } else if (options?.allowHeuristicFlagClear || nextConversation.messageCount <= 1) {
      nextConversation = { ...nextConversation, isFlagged: false };
    } else {
      incomplete = true;
    }
  }

  return { conversation: nextConversation, incomplete };
}

function summarizeConversation(conversation: ConversationView): ConversationSummary {
  const latestMessage = conversation.messages[conversation.messages.length - 1];
  return {
    id: conversation.id,
    subject: conversation.subject ?? latestMessage?.subject ?? null,
    preview: latestMessage?.preview ?? null,
    fromName: latestMessage?.fromName ?? null,
    fromEmail: latestMessage?.fromEmail ?? null,
    latestReceivedAt: latestMessage?.receivedAt ?? "",
    unreadCount: conversation.messages.reduce(
      (count, message) => count + (message.isRead ? 0 : 1),
      0,
    ),
    messageCount: conversation.messages.length,
    sourceIds: [...new Set(conversation.messages.map((message) => message.sourceId))],
    sourceNames: [...new Set(conversation.messages.map((message) => message.sourceName))],
    latestMessage: latestMessage
      ? { messageId: latestMessage.id, sourceId: latestMessage.sourceId }
      : { messageId: "", sourceId: "" },
    latestSourceName: latestMessage?.sourceName ?? "",
    hasAttachment: conversation.messages.some((message) => message.hasAttachment),
    isFlagged: conversation.messages.some((message) => message.isFlagged),
  };
}

function applyPatchToConversationView(
  conversation: ConversationView,
  target: MailSelection,
  patch: KeywordPatch,
): { changed: boolean; conversation: ConversationView; summary: ConversationSummary } {
  let changed = false;
  const messages = conversation.messages.map((message) => {
    if (message.sourceId !== target.sourceId || message.id !== target.messageId) {
      return message;
    }
    changed = true;
    return replaceMessageKeywords(message, patch.next);
  });

  const nextConversation = changed ? { ...conversation, messages } : conversation;
  return {
    changed,
    conversation: nextConversation,
    summary: summarizeConversation(nextConversation),
  };
}

export function applyKeywordPatch(
  queryClient: QueryClient,
  target: MailSelection,
  patch: KeywordPatch,
  options?: ReconcileOptions,
): CachePatchResult {
  const snapshots = [
    snapshotQuery(queryClient, mailKeys.message(target.sourceId, target.messageId)),
    snapshotQuery(queryClient, mailKeys.conversation(target.conversationId)),
    snapshotQuery(queryClient, mailKeys.conversationSummary(target.conversationId)),
  ];

  let incomplete = false;
  let exactSummary: ConversationSummary | null = null;

  const messageKey = mailKeys.message(target.sourceId, target.messageId);
  const currentMessage = queryClient.getQueryData<MessageDetail>(messageKey);
  if (currentMessage) {
    queryClient.setQueryData<MessageDetail>(
      messageKey,
      replaceMessageKeywords(currentMessage, patch.next),
    );
  } else {
    incomplete = true;
  }

  const conversationKey = mailKeys.conversation(target.conversationId);
  const currentConversation = queryClient.getQueryData<ConversationView>(conversationKey);
  if (currentConversation) {
    const updatedConversation = applyPatchToConversationView(
      currentConversation,
      target,
      patch,
    );
    queryClient.setQueryData(conversationKey, updatedConversation.conversation);
    exactSummary = updatedConversation.summary;
  }

  const currentSummary = getConversationSummary(queryClient, target.conversationId);
  if (exactSummary) {
    queryClient.setQueryData(
      mailKeys.conversationSummary(target.conversationId),
      currentSummary
        ? { ...currentSummary, ...exactSummary }
        : exactSummary,
    );
  } else if (currentSummary) {
    const heuristicResult = applyHeuristicConversationPatch(
      currentSummary,
      patch,
      options,
    );
    queryClient.setQueryData(
      mailKeys.conversationSummary(target.conversationId),
      heuristicResult.conversation,
    );
    incomplete ||= heuristicResult.incomplete;
  } else {
    incomplete = true;
  }

  return { incomplete, snapshots };
}

export function mergeMessageDetail(
  queryClient: QueryClient,
  detail: MessageDetail,
  conversationId: string,
) {
  queryClient.setQueryData(mailKeys.message(detail.sourceId, detail.id), detail);

  const conversationKey = mailKeys.conversation(conversationId);
  const conversation = queryClient.getQueryData<ConversationView>(conversationKey);
  if (!conversation) {
    return false;
  }

  const messages = conversation.messages.map((message) =>
    message.sourceId === detail.sourceId && message.id === detail.id
      ? replaceMessageKeywords(message, detail)
      : message,
  );
  const nextConversation = { ...conversation, messages };
  queryClient.setQueryData(conversationKey, nextConversation);

  const summary = summarizeConversation(nextConversation);
  const currentSummary = getConversationSummary(queryClient, conversationId);
  queryClient.setQueryData(
    mailKeys.conversationSummary(conversationId),
    currentSummary ? { ...currentSummary, ...summary } : summary,
  );

  return true;
}

export function findConversationIdForMessage(
  queryClient: QueryClient,
  target: SourceMessageRef,
): string | null {
  const cachedMessage = queryClient.getQueryData<MessageDetail>(
    mailKeys.message(target.sourceId, target.messageId),
  );
  if (cachedMessage) {
    return cachedMessage.conversationId;
  }

  for (const [, conversation] of queryClient.getQueriesData<ConversationView>({
    queryKey: ["conversation"],
  })) {
    if (
      conversation?.messages.some(
        (message) =>
          message.sourceId === target.sourceId && message.id === target.messageId,
      )
    ) {
      return conversation.id;
    }
  }

  for (const [, summary] of queryClient.getQueriesData<ConversationSummary>({
    queryKey: ["conversation-summary"],
  })) {
    if (
      summary?.latestMessage.sourceId === target.sourceId &&
      summary.latestMessage.messageId === target.messageId
    ) {
      return summary.id;
    }
  }

  return null;
}

export function applyKeywordEventPatch(
  queryClient: QueryClient,
  target: SourceMessageRef,
  keywords: string[],
): boolean {
  const conversationId = findConversationIdForMessage(queryClient, target);
  if (!conversationId) {
    return false;
  }

  const currentMessage = queryClient.getQueryData<MessageDetail>(
    mailKeys.message(target.sourceId, target.messageId),
  );
  if (!currentMessage) {
    return false;
  }

  applyKeywordPatch(
    queryClient,
    { ...target, conversationId },
    {
      next: deriveKeywordState(keywords),
      previous: deriveKeywordState(currentMessage.keywords),
    },
  );
  return true;
}

export function mergeConversationView(
  queryClient: QueryClient,
  conversation: ConversationView,
) {
  queryClient.setQueryData(mailKeys.conversation(conversation.id), conversation);
  queryClient.setQueryData(
    mailKeys.conversationSummary(conversation.id),
    summarizeConversation(conversation),
  );
}

export function readConversationIds(
  data: InfiniteData<ConversationPageSlice, unknown> | undefined,
): string[] {
  return data?.pages.flatMap((page) => page.itemIds) ?? [];
}
