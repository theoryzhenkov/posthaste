import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import wasm from 'vite-plugin-wasm'

// `wasm` lets the generated `posthaste-link-wasm` module (the client-layer
// replica boundary) load in the browser. Inert unless the replicaAdapter is
// imported (gated behind VITE_RUNTIME_REPLICA).
export default defineConfig({
  plugins: [react(), tailwindcss(), wasm()],
  resolve: {
    alias: {
      '@': resolve(import.meta.dirname, 'src'),
    },
  },
})
