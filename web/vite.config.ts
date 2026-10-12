import path from 'node:path'

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// In development the gateway runs on the host; in production nginx proxies /api.
const GATEWAY_URL = process.env.GATEWAY_URL ?? 'http://127.0.0.1:8080'

/**
 * Vendor chunks (CONTRACT.md §21): `react`, `query`, `ui` and `charts` are
 * named so they cache across deploys; shiki, mermaid, React Flow, the
 * editorial canvas and the markdown engine stay route-level lazy chunks
 * because only their pages import them (dynamically).
 */
function manualChunks(id: string): string | undefined {
  if (!id.includes('/node_modules/')) return undefined
  if (/\/node_modules\/(react|react-dom|react-router|scheduler)\//.test(id)) return 'react'
  if (id.includes('/node_modules/@tanstack/')) return 'query'
  if (/\/node_modules\/(radix-ui|@radix-ui|cmdk|lucide-react)\//.test(id)) return 'ui'
  if (id.includes('/node_modules/@dagrejs/')) return 'charts'
  return undefined
}

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
    rollupOptions: {
      output: { manualChunks },
    },
  },
})
