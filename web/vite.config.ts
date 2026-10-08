import path from 'node:path'

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// In development the gateway runs on the host; in production nginx proxies /api.
const GATEWAY_URL = process.env.GATEWAY_URL ?? 'http://127.0.0.1:8080'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': path.resolve(import.meta.dirname, './src') },
  },
  server: {
    port: 5173,
    strictPort: true,
    proxy: {
      '/api': { target: GATEWAY_URL, changeOrigin: true },
    },
  },
  build: {
    target: 'es2023',
    sourcemap: false,
  },
})
