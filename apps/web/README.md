# mail web UI

React 19 + Vite frontend for the local mail daemon. The app talks only to the Rust API at `http://localhost:3001/v1` and receives live updates from the daemon's EventSource stream at `/v1/events`.

## Commands

```bash
bun install
bun run dev
bun run build
bun run check
```

## Architecture

- `src/App.tsx` owns the shell, toolbar, search box, selected view, selected message, and settings panel toggle.
- `src/components/Sidebar.tsx` renders smart mailboxes plus enabled account mailboxes from the `["sidebar"]` query.
- `src/components/MessageList.tsx` is the main thread list. It uses `useInfiniteQuery`, cursor-based conversation pagination, fixed-row virtualization, per-view scroll restoration, and anchored prepends for new mail arriving at the top.
- `src/components/MessageDetail.tsx` fetches the selected conversation and selected message detail separately, then renders a per-message switcher plus the chosen body.
- `src/components/EmailFrame.tsx` renders sanitized HTML in a fixed-height iframe viewport. The iframe scrolls internally; the app no longer tries to auto-size long newsletter bodies to full document height.
- `src/hooks/useDaemonEvents.ts` opens the EventSource connection, persists the last seen event sequence in `sessionStorage`, dispatches browser `CustomEvent`s for list-level live updates, and batches React Query invalidations for sidebar and detail data.
- `src/hooks/useEmailActions.ts` issues message mutations and invalidates the affected conversation queries after local actions.

## Conversation list behavior

- Conversation rows are fetched in pages of `100`.
- Pagination is seek-based, using an opaque cursor from the backend rather than `OFFSET`.
- The list renders only the visible rows plus overscan, based on a fixed `ROW_HEIGHT`.
- When new conversations arrive, they are inserted at the top. If the user is scrolled away from the top, `scrollTop` is compensated so the viewport stays anchored on the same visible content.

## Live update model

- The daemon exposes a single ordered domain event stream.
- The frontend reconnects with `afterSeq` so it can resume without replaying the entire backlog.
- Sidebar, message detail, and selected message caches are invalidated in short batches.
- The conversation list handles its own top-of-list refresh and merge path instead of broad invalidation, because it is paginated and virtualized.
