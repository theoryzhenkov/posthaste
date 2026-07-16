import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// HTTP+SSE on localhost is THE transport: the client runs identically in a
// bare browser tab; Tauri arrives later as a thin shell (window/tray/updater),
// never as a transport. Once the backend lands, its command + stream endpoints
// are proxied here for the dev loop:
//
//   server: { proxy: { '/api': 'http://127.0.0.1:<port>' } }
export default defineConfig({
  plugins: [react()],
})
