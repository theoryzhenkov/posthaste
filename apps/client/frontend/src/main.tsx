import { createRoot } from 'react-dom/client'

// Scaffolding only — the mirror store (subscribe → apply patch → render) and
// its first surface arrive once the protocol shapes in `models/` are decided.
createRoot(document.getElementById('root')!).render(
  <p>posthaste client — scaffolding only, implementation pending</p>,
)
