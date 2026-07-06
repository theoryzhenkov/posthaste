import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import wasm from 'vite-plugin-wasm'

// `wasm` lets the generated `posthaste-client-node-wasm` module (the client-layer
// replica boundary) load in the browser. The entity-store replica adapter is
// always installed, so the WASM module is a required part of every bundle.
export default defineConfig({
  plugins: [react(), tailwindcss(), wasm()],
  resolve: {
    alias: {
      '@': resolve(import.meta.dirname, 'src'),
    },
  },
})
