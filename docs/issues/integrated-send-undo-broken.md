---
title: "Send and undo broken in the integrated app"
modified: 2026-07-18
state: open
---

# Send and undo broken in the integrated app

Observed in dogfood on v0.6.0-nightly.1: composing + sending does not
deliver, and undo does not act. The outbox core (enqueue/fold/flush/settle,
undo-window readiness) is heavily covered at L2/L3 and the live send path
passes against Stalwart, so suspicion falls on the new command wiring:
the send verb's envelope (hold fields), the compose submit path in the
ported UI, or the undo verb's mapping onto the rev_log/outbox surface.

Deliberately deferred behind the legacy retirement (code quality first).
Reproduce in the dev loop (backend example + client:dev), then bisect:
UI verb -> /api/command envelope -> outbox row -> flush.
