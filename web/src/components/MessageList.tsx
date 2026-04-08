/**
 * Paginated, virtualized conversation list with live prepend and keyboard navigation.
 *
 * Uses manual fixed-row virtualization (no library), seek-based cursor pagination,
 * and anchored scroll adjustment on live prepends.
 *
 * @spec docs/L1-ui#messagelist
 * @spec docs/L1-ui#keyboard-shortcuts
 */
import {
  DndContext,
  PointerSensor,
  closestCenter,
  useSensor,
  useSensors,
  type DragEndEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  arrayMove,
  horizontalListSortingStrategy,
} from "@dnd-kit/sortable";
import {
  useInfiniteQuery,
  useQueryClient,
  useQueries,
  type InfiniteData,
} from "@tanstack/react-query";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  fetchConversations,
  fetchSmartMailboxConversations,
} from "../api/client";
import type {
  ConversationSummary,
  DomainEvent,
} from "../api/types";
import type { EmailActions } from "../hooks/useEmailActions";
import { MAIL_DOMAIN_EVENT_NAME } from "../hooks/useDaemonEvents";
import {
  getConversationSummary,
  mailKeys,
  normalizeConversationPage,
  readConversationIds,
  type ConversationPageSlice,
  type MailSelection,
} from "../mailState";
import { AlertCircle, Inbox, MousePointerClick } from "lucide-react";
import type { SidebarSelection } from "./Sidebar";
import { MessageRow } from "./MessageRow";
import { ColumnPickerMenu } from "./thread-list/ColumnPickerMenu";
import { SortableColumnHeader } from "./thread-list/SortableColumnHeader";
import {
  type ColumnId,
  type SortConfig,
  buildGridTemplate,
  getColumnDef,
} from "./thread-list/columns";
import { useColumnConfig } from "./thread-list/useColumnConfig";

/** @spec docs/L1-ui#messagelist */
interface MessageListProps {
  selectedView: SidebarSelection | null;
  selection: MailSelection | null;
  onSelectMessage: (message: MailSelection) => void;
  actions: EmailActions;
}

/** @spec docs/L1-ui#messagelist */
const PAGE_SIZE = 100;
/** @spec docs/L1-ui#messagelist */
const ROW_HEIGHT = 78;
const OVERSCAN_ROWS = 6;
const LOAD_MORE_THRESHOLD_PX = 800;
const TOP_REFRESH_THRESHOLD_PX = 24;
/** Per-view scroll offset cache to restore position on view switch. */
const scrollOffsetByView = new Map<string, number>();

/**
 * Fetch a conversation page for the currently selected sidebar view,
 * routing to the appropriate API endpoint.
 * @spec docs/L1-api#cursor-pagination
 */
function fetchConversationPageForView(
  selectedView: SidebarSelection,
  cursor: string | null,
  sort: SortConfig,
) {
  const sortParams = { sort: sort.columnId, sortDir: sort.direction };
  if (selectedView.kind === "smart-mailbox") {
    return fetchSmartMailboxConversations(selectedView.id, {
      limit: PAGE_SIZE,
      cursor,
      ...sortParams,
    });
  }
  return fetchConversations({
    sourceId: selectedView.sourceId,
    mailboxId: selectedView.mailboxId,
    limit: PAGE_SIZE,
    cursor,
    ...sortParams,
  });
}

/** Stable string key for a sidebar selection, used for scroll-offset caching. */
function conversationViewKey(selectedView: SidebarSelection | null): string {
  if (!selectedView) {
    return "none";
  }
  if (selectedView.kind === "smart-mailbox") {
    return `smart:${selectedView.id}`;
  }
  return `source:${selectedView.sourceId}:${selectedView.mailboxId}`;
}

/** Check whether an SSE event could affect the currently displayed view. */
function eventMayAffectView(
  payload: DomainEvent,
  selectedView: SidebarSelection | null,
): boolean {
  if (!selectedView) {
    return false;
  }
  if (selectedView.kind === "smart-mailbox") {
    return true;
  }
  if (payload.accountId !== selectedView.sourceId) {
    return false;
  }
  return payload.mailboxId === null || payload.mailboxId === selectedView.mailboxId;
}

/**
 * Conversation list panel: the middle column of the three-column layout.
 *
 * Handles pagination, manual virtualization, live prepend on domain events,
 * per-view scroll restoration, and keyboard shortcuts (j/k, arrows, archive, trash).
 *
 * @spec docs/L1-ui#messagelist
 * @spec docs/L1-ui#live-prepend-behavior
 * @spec docs/L1-ui#keyboard-shortcuts
 */
export function MessageList({
  selectedView,
  selection,
  onSelectMessage,
  actions,
}: MessageListProps) {
  const queryClient = useQueryClient();
  const { columns, sort, widths, toggleColumn, reorderColumns, resetColumns, toggleSort, setColumnWidth } =
    useColumnConfig();
  const dndSensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 5 } }),
  );
  const queryKey = useMemo(
    () => mailKeys.view(selectedView, sort),
    [selectedView, sort],
  );
  const viewKey = useMemo(() => conversationViewKey(selectedView), [selectedView]);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const restoredViewKeyRef = useRef<string | null>(null);
  const refreshInFlightRef = useRef(false);
  const refreshQueuedRef = useRef(false);
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [isRefreshingTop, setIsRefreshingTop] = useState(false);

  const {
    data,
    isLoading,
    isFetching,
    isFetchingNextPage,
    hasNextPage,
    fetchNextPage,
    refetch,
    error,
  } = useInfiniteQuery({
    queryKey,
    queryFn: async ({ pageParam }) =>
      normalizeConversationPage(
        queryClient,
        await fetchConversationPageForView(selectedView!, pageParam, sort),
      ),
    initialPageParam: null as string | null,
    getNextPageParam: (lastPage) => lastPage.nextCursor,
    enabled: selectedView !== null,
  });

  const conversationIds = useMemo(() => readConversationIds(data), [data]);

  function handleColumnDragEnd(event: DragEndEvent) {
    const { active, over } = event;
    if (over && active.id !== over.id) {
      const oldIndex = columns.indexOf(active.id as ColumnId);
      const newIndex = columns.indexOf(over.id as ColumnId);
      reorderColumns(arrayMove(columns, oldIndex, newIndex));
    }
  }

  /**
   * Refetch the first page and prepend any new conversations, adjusting
   * `scrollTop` to keep the visible viewport anchored.
   * @spec docs/L1-ui#live-prepend-behavior
   */
  const refreshFirstPage = useCallback(async () => {
    if (!selectedView) {
      return;
    }
    if (refreshInFlightRef.current) {
      refreshQueuedRef.current = true;
      return;
    }

    refreshInFlightRef.current = true;
    setIsRefreshingTop(true);
    try {
      const fetchedPage = await fetchConversationPageForView(selectedView, null, sort);
      const firstPage = normalizeConversationPage(queryClient, fetchedPage);
      let insertedCount = 0;
      queryClient.setQueryData<InfiniteData<ConversationPageSlice, string | null>>(queryKey, (current) => {
        if (!current || current.pages.length === 0) {
          insertedCount = firstPage.itemIds.length;
          return {
            pages: [firstPage],
            pageParams: [null],
          };
        }

        const loadedIds = current.pages.flatMap((page) => page.itemIds);
        const currentTopConversationId = loadedIds[0] ?? null;
        const prependedIds: string[] = [];
        for (const itemId of firstPage.itemIds) {
          if (currentTopConversationId !== null && itemId === currentTopConversationId) {
            break;
          }
          prependedIds.push(itemId);
        }
        insertedCount = prependedIds.length;

        const prependedIdSet = new Set(prependedIds);
        const pages = current.pages.map((page, index) => {
          const retainedIds = page.itemIds.filter((itemId) => !prependedIdSet.has(itemId));

          if (index === 0) {
            return {
              ...page,
              itemIds: [...prependedIds, ...retainedIds],
              nextCursor: firstPage.nextCursor,
            };
          }

          return {
            ...page,
            itemIds: retainedIds,
          };
        });

        return {
          ...current,
          pages,
        };
      });

      if (insertedCount > 0 && scrollTop > TOP_REFRESH_THRESHOLD_PX && scrollContainerRef.current) {
        const nextScrollTop = scrollContainerRef.current.scrollTop + insertedCount * ROW_HEIGHT;
        scrollContainerRef.current.scrollTop = nextScrollTop;
        scrollOffsetByView.set(viewKey, nextScrollTop);
        setScrollTop(nextScrollTop);
      }
    } finally {
      refreshInFlightRef.current = false;
      setIsRefreshingTop(false);
      if (refreshQueuedRef.current) {
        refreshQueuedRef.current = false;
        void refreshFirstPage();
      }
    }
  }, [queryClient, queryKey, scrollTop, selectedView, sort, viewKey]);

  /** Move selection to the next or previous conversation. */
  const navigateMessage = useCallback(
    (direction: 1 | -1) => {
      if (conversationIds.length === 0) return;

      const currentIndex = conversationIds.findIndex(
        (conversationId) => conversationId === selection?.conversationId,
      );
      const nextIndex =
        currentIndex === -1
          ? direction === 1
            ? 0
            : conversationIds.length - 1
          : currentIndex + direction;

      if (nextIndex < 0) {
        return;
      }
      if (nextIndex >= conversationIds.length) {
        if (direction === 1 && hasNextPage && !isFetchingNextPage) {
          void fetchNextPage();
        }
        return;
      }

      const nextConversationId = conversationIds[nextIndex];
      const nextConversation = getConversationSummary(queryClient, nextConversationId);
      if (!nextConversation) {
        return;
      }
      onSelectMessage({
        conversationId: nextConversation.id,
        sourceId: nextConversation.latestMessage.sourceId,
        messageId: nextConversation.latestMessage.messageId,
      });
    },
    [
      conversationIds,
      queryClient,
      fetchNextPage,
      hasNextPage,
      isFetchingNextPage,
      onSelectMessage,
      selection?.conversationId,
    ],
  );

  // Keyboard shortcuts -- suppressed when an input has focus.
  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      const target = event.target as HTMLElement;
      if (target.tagName === "INPUT" || target.tagName === "TEXTAREA") return;

      switch (event.key) {
        case "j":
        case "ArrowDown":
          event.preventDefault();
          navigateMessage(1);
          break;
        case "k":
        case "ArrowUp":
          event.preventDefault();
          navigateMessage(-1);
          break;
        case "e":
          if (selection) {
            actions.archive({ sourceId: selection.sourceId, messageId: selection.messageId });
          }
          break;
        case "#":
        case "Backspace":
          if (selection) {
            actions.trash({ sourceId: selection.sourceId, messageId: selection.messageId });
          }
          break;
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [actions, navigateMessage, selection]);

  // Reset scroll-restore tracking on view change.
  useEffect(() => {
    restoredViewKeyRef.current = null;
  }, [viewKey]);

  // Restore scroll position when switching views.
  useEffect(() => {
    const node = scrollContainerRef.current;
    if (!node || restoredViewKeyRef.current === viewKey) {
      return;
    }
    const savedOffset = scrollOffsetByView.get(viewKey) ?? 0;
    restoredViewKeyRef.current = viewKey;
    node.scrollTop = savedOffset;
    setScrollTop(savedOffset);
  }, [viewKey, conversationIds.length]);

  // Track viewport height for virtualization.
  useEffect(() => {
    const node = scrollContainerRef.current;
    if (!node) {
      return;
    }

    const updateViewportHeight = () => setViewportHeight(node.clientHeight);
    updateViewportHeight();

    const resizeObserver = new ResizeObserver(updateViewportHeight);
    resizeObserver.observe(node);
    return () => resizeObserver.disconnect();
  }, []);

  // Fetch next page when near the bottom.
  useEffect(() => {
    const node = scrollContainerRef.current;
    if (!node || !hasNextPage || isFetchingNextPage) {
      return;
    }

    const remaining = node.scrollHeight - (node.scrollTop + node.clientHeight);
    if (remaining <= LOAD_MORE_THRESHOLD_PX) {
      void fetchNextPage();
    }
  }, [
    conversationIds.length,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    scrollTop,
    viewportHeight,
  ]);

  // Listen for domain events and refresh the first page.
  useEffect(() => {
    function handleDomainEvent(event: Event) {
      const payload = (event as CustomEvent<DomainEvent>).detail;
      if (!eventMayAffectView(payload, selectedView)) {
        return;
      }
      void refreshFirstPage();
    }

    window.addEventListener(MAIL_DOMAIN_EVENT_NAME, handleDomainEvent as EventListener);
    return () =>
      window.removeEventListener(MAIL_DOMAIN_EVENT_NAME, handleDomainEvent as EventListener);
  }, [refreshFirstPage, scrollTop, selectedView]);

  const handleScroll = useCallback(() => {
    const node = scrollContainerRef.current;
    if (!node) {
      return;
    }
    setScrollTop(node.scrollTop);
    scrollOffsetByView.set(viewKey, node.scrollTop);
  }, [viewKey]);

  const totalRows = conversationIds.length;
  const safeViewportHeight = viewportHeight || ROW_HEIGHT * 8;
  const startIndex = Math.max(
    0,
    Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN_ROWS,
  );
  const endIndex = Math.min(
    totalRows,
    Math.ceil((scrollTop + safeViewportHeight) / ROW_HEIGHT) + OVERSCAN_ROWS,
  );
  const topSpacerHeight = startIndex * ROW_HEIGHT;
  const bottomSpacerHeight = (totalRows - endIndex) * ROW_HEIGHT;
  const visibleConversationIds = conversationIds.slice(startIndex, endIndex);
  const visibleConversations = useQueries({
    queries: visibleConversationIds.map((conversationId) => ({
      queryKey: mailKeys.conversationSummary(conversationId),
    })),
    combine: (results) =>
      results
        .map((result) => result.data)
        .filter((conversation): conversation is ConversationSummary => !!conversation),
  });
  const countLabel = hasNextPage ? `${totalRows} loaded` : `${totalRows} threads`;

  if (!selectedView) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 border-r border-border bg-background p-6">
        <MousePointerClick size={40} strokeWidth={1.5} className="text-muted-foreground/40" />
        <div className="text-center">
          <p className="text-sm font-medium text-muted-foreground">No mailbox selected</p>
          <p className="mt-1 text-xs text-muted-foreground/60">Pick a mailbox to get started</p>
        </div>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col overflow-hidden border-r border-border bg-background">
      <div className="border-b border-border pt-3">
        <div className="flex items-baseline gap-2 px-3">
          <h2 className="text-sm font-semibold tracking-tight">{selectedView.name}</h2>
          <span className="text-xs text-muted-foreground">{countLabel}</span>
        </div>

        <ColumnPickerMenu
          activeColumns={columns}
          onToggle={toggleColumn}
          onReset={resetColumns}
        >
          <div
            className="mt-2 grid gap-3 border-t border-border px-3 pb-2 pt-1.5 text-[10px] font-mono uppercase tracking-wider text-muted-foreground"
            style={{ gridTemplateColumns: buildGridTemplate(columns, widths) }}
          >
            <DndContext
              sensors={dndSensors}
              collisionDetection={closestCenter}
              onDragEnd={handleColumnDragEnd}
            >
              <SortableContext
                items={columns}
                strategy={horizontalListSortingStrategy}
              >
                {columns.map((colId) => {
                  const def = getColumnDef(colId);
                  return (
                    <SortableColumnHeader
                      key={colId}
                      id={colId}
                      label={def.label}
                      align={def.align}
                      sortDirection={
                        sort.columnId === colId ? sort.direction : undefined
                      }
                      onSort={() => toggleSort(colId)}
                      onResize={(w) => setColumnWidth(colId, w)}
                    />
                  );
                })}
              </SortableContext>
            </DndContext>
          </div>
        </ColumnPickerMenu>
      </div>

      <div
        ref={scrollContainerRef}
        className="min-h-0 flex-1 overflow-y-auto"
        onScroll={handleScroll}
      >
        {isLoading && (
          <div className="space-y-0">
            {Array.from({ length: 4 }).map((_, i) => (
              <div
                key={i}
                className="border-b border-border px-3 py-4"
                style={{ height: ROW_HEIGHT }}
              >
                <div className="flex items-center gap-3">
                  <div className="h-3.5 w-28 animate-pulse rounded bg-muted" />
                  <div className="h-3 w-16 animate-pulse rounded bg-muted" />
                </div>
                <div className="mt-2.5 h-3 w-3/4 animate-pulse rounded bg-muted" />
                <div className="mt-2 h-3 w-1/2 animate-pulse rounded bg-muted/60" />
              </div>
            ))}
          </div>
        )}
        {error && (
          <div className="flex flex-col items-center gap-3 px-3 py-12">
            <AlertCircle size={32} strokeWidth={1.5} className="text-destructive/50" />
            <p className="text-sm text-destructive">Failed to load threads</p>
            <button
              type="button"
              className="rounded border border-border px-3 py-1.5 text-xs text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              onClick={() => void refetch()}
            >
              Try again
            </button>
          </div>
        )}
        {!isLoading && !error && conversationIds.length === 0 && (
          <div className="flex flex-col items-center gap-3 px-3 py-12">
            <Inbox size={40} strokeWidth={1.5} className="text-muted-foreground/40" />
            <div className="text-center">
              <p className="text-sm font-medium text-muted-foreground">No threads here yet</p>
              <p className="mt-1 text-xs text-muted-foreground/60">
                Messages will appear as they arrive
              </p>
            </div>
          </div>
        )}
        {conversationIds.length > 0 && (
          <>
            <div style={{ height: topSpacerHeight }} />
            {visibleConversations.map((conversation) => (
              <div key={conversation.id} style={{ height: ROW_HEIGHT }}>
                <MessageRow
                  message={conversation}
                  isSelected={conversation.id === selection?.conversationId}
                  columns={columns}
                  widths={widths}
                  onSelect={() =>
                    onSelectMessage({
                      conversationId: conversation.id,
                      sourceId: conversation.latestMessage.sourceId,
                      messageId: conversation.latestMessage.messageId,
                    })
                  }
                />
              </div>
            ))}
            <div style={{ height: bottomSpacerHeight }} />
          </>
        )}
        {(isFetchingNextPage || isRefreshingTop) && (
          <p className="px-3 py-3 text-xs text-muted-foreground">
            {isRefreshingTop ? "Refreshing threads..." : "Loading more threads..."}
          </p>
        )}
        {!hasNextPage && conversationIds.length > 0 && !isFetching && (
          <p className="px-3 py-3 text-xs text-muted-foreground">End of thread list</p>
        )}
      </div>
    </div>
  );
}
