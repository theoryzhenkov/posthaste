---
id: performance
from: Theo
subject: Performance
tag: onboarding
time: '08:52'
color: sage
unread: true
---

Posthaste comes with a powerful internal query language, optimised over an SQLite database. Smart mailboxes & search use it as a backend. Agressive indexing allows us to achieve sub 300 ms query times over the entire message repository. I further optimise performance with a complex orchestration of optimistic mutations, smart predictive caching, compiled WASM modules, and other tricks.

If you run into a performance or stability issue, file a report via [GitHub](https://github.com/theoryzhenkov/posthaste/issues), [Mail](mailto:proj+posthaste@theor.net) or [Discord](https://discord.gg/8ARFrDa2Gv).
