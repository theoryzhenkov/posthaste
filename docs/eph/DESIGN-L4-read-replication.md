---
scope: L4
summary: "A split runtime serves reads through one coherent read-through cache shared with the client and parameterized by a link-cost policy: the backend authority computes queries, each near node caches the data that flowed back and reads through on a miss; co-located the policy is passthrough, so there is no redundant cache"
modified: 2026-06-24
reviewed: 2026-06-24
lifecycle: ephemeral
type: DESIGN
depends:
  - path: docs/replication/L4
    section: "4.3 One replica, two consumers"
  - path: docs/replication/L4
    section: "3. The link contract"
  - path: docs/replication/L2
    section: "4. The replica node"
---

# Read replication: the runtime as a read-through cache (L4 W4)

## 1. The problem

The runtime↔backend **write** path is proven end to end (W3c c3): a `Remote`
runtime forwards a mutation across the link and the backend's store applies it.
The **read** path is not. A `Remote` runtime still computes its views from its
*local* `mail_queries` over a *local* store — empty in a real split. The c3 test
had to use `message.destroy` because `setFlaggedState` needs a local read
(undo-history) the split runtime cannot do: `NotFound: message:m-split`.

So everything the runtime reads — view rows, message detail, the current-state
reads behind undo-history — must reach the backend when the two are split.

## 2. The model: one read-through cache, policy per link

Each near node holds **one shared component**: a coherent cache + outbox over its
far node, parameterized by a **caching policy** chosen from the **cost of its
link**. This is a progressive cache hierarchy, not a uniform replica:

```
providers → backend (authority; store caches what it synced)
          → runtime (caches what it fetched; disk, aggressive policy)
          → client  (minimal replica; visible window, thin policy)
```

The primitive is **read-through**; caching is an **optimization** over it:

- A read is served from cache on a hit, or **read through to the far node** on a
  miss. The cached entry is then kept coherent by the down-channel.
- The **policy** decides what to retain and for how long, balancing storage
  against round-trips. It ranges from **passthrough** (retain nothing, always
  read through) to **aggressive** (retain everything fetched).
- The policy is a function of link cost. An **instant link → passthrough**: cache
  nothing because reading through costs nothing.

The consequence that makes this clean: **co-located is passthrough, so there is
no redundant cache and behavior is exactly today's — by construction, not as a
special case.** Remote pays for a slow link by retaining what it fetches.

## 3. What is cached: data, not the ability to answer queries

The **query engine stays at the authority**. Filtering, sorting, and paginating a
mail list (`mail_queries`) runs at the backend, which owns the whole store. A new
list query or a new page is a read-through.

What a near node **caches is the data that flowed back** — the rows a query
returned, the messages a detail read fetched. So:

- **Re-serving a known list** and **point reads** (detail, undo-history's
  current state) are local once the data is cached.
- **A new query / page** is a read-through to the authority.

The runtime therefore caches *messages and rows*, not a query engine. This is the
progressive-cache line between "compute at the authority" and "cache what
flowed back," and it is what keeps a near node from replicating the store (the
Model B we reject — replicate every record and run the queries locally).

## 4. Read-through dissolves the point-read question

A point read the cache cannot satisfy is **a read-through on a miss** — the same
primitive as the co-located read, paid over the link. There is no separate
"point-read channel" and no assertion to relax: reads are read-through with a
policy cache, and co-located the cache is empty.

Undo-history then needs no special case: the message it reads is usually already
cached (the runtime fetched it when it appeared in a list); on a miss it reads
through; offline-and-uncached it is simply unavailable (you cannot undo what you
never saw). `backend-link-is-replication-only` is restated accordingly:

> The runtime's served reads are read-through over the link with a policy-driven
> coherent cache; co-located the policy is passthrough (no cache). Writes forward
> up the link; nothing reaches across it but the read-through and the mutation.

## 5. The cache is the coherent base, not a TTL cache

A cached row/message is not held until it expires; it is held until the
down-channel says it changed. The convergence engine already keeps the
**overlay** coherent (W3c); the cache extends the same base it folds over. So:

- The down-channel delivers authoritative updates for cached entries (the
  view-frame protocol for subscribed lists; base assertions for messages).
- **Eviction** drops an entry from the coherent set; the next read re-fetches it.
  Eviction is a policy decision (storage pressure), not a coherence one.

"Progressive" is *how much* of the coherent base each layer retains — everything
fetched (runtime) versus the visible window (client) — under one mechanism.

## 6. Co-located is the same code, collapsed

Today's runtime plays both roles in one process: it computes views (authority
role) and serves a session (near-node role) over one store. W4 is the seam
between them, as W1 was on the write path — but the seam is **one cache with a
policy**, not two implementations:

- **`ReadSource`** — the far node's read surface: `query_mail_page`,
  `message_detail`, `current_summary`, `conversation`. `LocalReadSource` calls
  the in-process backend directly (today's reads). `RemoteReadSource` calls over
  the link.
- **`ReadCache`** wraps a `ReadSource` with a policy. **Passthrough** (the
  default, co-located) delegates every read straight through — no storage, no
  copy, behavior-preserving. A retaining policy caches what flowed back and
  serves hits locally.
- The runtime's `ViewRegistry` and read methods draw from the `ReadCache`. They
  already **cache the overlay** (W3c `MailListReplica` + outbox); the read base
  now comes from the source through the policy.

The link's read channel is the **view-frame protocol** the runtime already
speaks to its client (`ViewSnapshot` / `ViewReplace`), recognized as the link's
read half ([L4 §4.1](../replication/L4.md)). `InProcessTransport` wires it to the
backend's computation directly; `RemoteTransport` carries it as HTTP + SSE.

## 7. Slicing (W4)

- **W4a — the read seam (passthrough).** *(Landed.)* `read.rs`: the `ReadSource`
  trait (`query_mail_page` + `current_summary`), `LocalReadSource` over
  `mail_queries` + service, and a passthrough `ReadCache`. The runtime's
  mail-list base (`ViewRegistry::build_snapshot`) and the point read behind
  undo-history (`current_message_summary` — the c3 blocker) draw from the
  `ReadCache`. Passthrough retains nothing, so the co-located deployment is
  unchanged (64 unit + 28 integration + all server suites pass). Mirrors W1.
- **W4b — backend read surface.** The far node serves the `ReadSource` methods
  (queries + point reads) and the view-subscription protocol; `LinkTransport`
  grows the read half. In-process first; co-located unchanged.
- **W4c — `RemoteReadSource` + retaining policy + `link_router` reads.** A split
  runtime reads through over the link and retains what it fetched; the split
  test now serves a **read** from the backend (the c3 twin). The
  `MailListReplica` caches served rows, the outbox folds, the client is served.
- **W4d — coherence + eviction + coverage.** The down-channel keeps cached
  entries live; eviction under storage pressure; `RuntimeCoverage` reports what a
  split runtime holds; reconnect/snapshot recovery.
- **W4e — policy surface + control config.** Expose the policy (static:
  passthrough vs retaining) in the desktop control config alongside the per-link
  transport switch; the adaptive RTT-driven policy is a later refinement, not
  built now.

## 8. Open questions

1. **Compute: move or call in place?** The end state is the backend *owning*
   query computation. W4a/b keep that code where it is and merely *call* it as the
   authority role (`LocalReadSource`); a crate-level relocation waits until a
   real split deployment needs it. Recommend: split by role now, relocate later.
2. **Coverage granularity.** Does the split runtime read through per request and
   retain per its policy, or pre-subscribe to a working set (an account's recent
   mail) it keeps warm? Per-request read-through + retain is simplest and falls
   straight out of §2; a warm working set is a policy refinement. Recommend
   per-request to start.
3. **Detail / conversation as cache entries or replicas?** Single-object views
   are point reads cached as data (§3); they get a folding replica only if
   optimistic offline *detail* is wanted. Recommend: cache as data first.
