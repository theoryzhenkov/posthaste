---
scope: L2
summary: "Reused/leftover structures around the reactive store that are fragile or actively storm during sync: the mail-list query still mounts a legacy REST useInfiniteQuery on the SAME key the store writes frames to (disarmed but a loaded gun); legacy domain-cache invalidations (mail-navigation bootstrap, target message) fire UNGATED on every message.updated (refetch storm during sync); dead useDomainEventRefresh; and rejection has no user-facing surface."
modified: 2026-06-27
reviewed: 2026-06-27
lifecycle: ephemeral
type: ISSUE
status: open
priority: medium
depends:
  - path: docs/eph/DESIGN-L2-client-link-reactive-store
---

# Legacy / leftover structures around the store

The store sits in front of the *old* HTTP + domain-event machinery rather than
replacing it. Several of those reused structures are fragile or counter-productive.

## A — Dual-path mail-list query on one key (fragile)

`apps/web/src/components/MessageList.tsx:115` mounts a legacy REST
`useInfiniteQuery` (with a `queryFn`) on `messageQueryKey`, **and** the frame-
writer (`useRuntimeMailListView`) writes to the **same key** via `setQueryData`
(`:170`). It's currently disarmed — the infinite query is `enabled: … &&
!useRuntimeViewFrames`, so its `queryFn` doesn't run while frames are on — but it
is a loaded gun: any regression in that `enabled` flag instantly makes REST
refetches (focus/reconnect/invalidate) clobber every frame's optimism. Verified
*not* the current flicker source (disabled), but it's the structure to retire.

**Fix:** once the store is trusted, delete the legacy infinite query + its
`fetchMessagesForView` path for the runtime-view case; stop sharing the key with
a REST fetcher.

## B — Ungated legacy invalidations storm during sync (likely count/sidebar churn)

On every `MessageUpdated`, even with the store active, these fire **without**
`skipStoreOwned` (`apps/web/src/domain-cache/handlers.ts`):
`invalidateMailNavigationBootstrapReadModels` (on arrived/mailboxes/keywords/
deleted) and `invalidateTargetMessageReadModels` (always). During sync that's a
refetch storm on the sidebar/nav and the open message — a likely source of
**count/sidebar** churn even though the list rows are protected (the user did not
report count flicker, but this is the mechanism if it appears). Smart-mailbox /
conversations invalidations also fire from other handlers.

**Fix:** audit each ungated invalidation against what the store now owns; gate
the ones whose surface the store maintains (`setQueryData` counts), keep only the
genuinely store-external ones (conversations, smart-mailboxes, detail body).

## C — Dead `useDomainEventRefresh` (cleanup)

`useDomainEventRefresh` + `eventMayAffectView` are gated off since
`runtimeMailListViewsEnabled` went default-on (`MessageList.tsx:182`,
`enabled: !useRuntimeViewFrames`) — dead for the default path. Remove.

## D — Rejection is invisible to the user (UX)

`mutationNotification`/`rejected` only `syncLogger.warn` and reverts
(`entityStoreAdapter.ts:349`); the user's action visibly undoes with no toast,
unlike `OperationSettled` failures which `pushNotification`. Synchronous run
rejection (`runMutation` catch) is rethrown but the revert still has no UI.

**Fix:** `pushNotification({severity:'error', …})` on the rejected branch,
surfacing `error.message`/`error.retryable`.

## Provenance

A/B from the flicker-investigation round (2026-06-27); D from four-reviewer
Task 2/Task 3 (MEDIUM-5); C is a standing cleanup from the 2e/2f follow-ups.
