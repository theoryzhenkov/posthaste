import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'
import { defineConfig } from 'vite'

// HTTP+SSE on localhost is THE transport: the client runs identically in a
// bare browser tab; Tauri is a thin shell, never a transport.
//
// Dev loop: start the backend on 127.0.0.1:7365, export its session secret as
// POSTHASTE_DEV_TOKEN, then `bun run client:dev`. The proxy forwards /api and
// /events to the backend and injects the Authorization header, so the page in
// the browser needs no token of its own (EventSource cannot set headers). The
// packaged app talks to the backend directly, constructing the MailClient
// from the connection-info file.
const backend = 'http://127.0.0.1:7365'

export default defineConfig(() => {
  const token = process.env.POSTHASTE_DEV_TOKEN
  const headers = token ? { Authorization: `Bearer ${token}` } : undefined
  return {
    plugins: [react(), tailwindcss()],
    resolve: {
      alias: {
        '@': resolve(import.meta.dirname, 'src'),
      },
    },
    server: {
      proxy: {
        '/api': { target: backend, headers },
        '/events': { target: backend, headers },
      },
    },
  }
})
