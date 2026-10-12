import path from 'node:path'

import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// In development the gateway runs on the host; in production nginx proxies /api.
const GATEWAY_URL = process.env.GATEWAY_URL ?? 'http://127.0.0.1:8080'

type ModuleInfoLike = { isEntry: boolean; importers: readonly string[] } | null
type GetModuleInfo = (id: string) => ModuleInfoLike

/**
 * Whether a module is reachable from the entry through static imports only.
 * Memoised per build; a module that is only ever imported by a lazy route
 * chunk must not be forced into an initial vendor chunk.
 */
const initialCache = new Map<string, boolean>()
function isInitial(id: string, getModuleInfo: GetModuleInfo, path: Set<string> = new Set()): boolean {
  const cached = initialCache.get(id)
  if (cached !== undefined) return cached
  if (path.has(id)) return false
  path.add(id)
  const info = getModuleInfo(id)
  const result = info ? info.isEntry || info.importers.some((importer) => isInitial(importer, getModuleInfo, path)) : false
  path.delete(id)
  initialCache.set(id, result)
  return result
}

/**
 * Vendor chunks (CONTRACT.md §21): `react`, `query`, `ui` and `charts` are
 * named so they cache across deploys; shiki, mermaid, React Flow, the
 * editorial canvas and the markdown engine stay route-level lazy chunks
 * because only their pages import them (dynamically).
 *
 * `ui` only takes the primitives the shell itself imports statically.
 * Radix packages, cmdk and lucide icons that only a lazy route needs stay
 * with that route, so the initial JS does not carry every dialog, radio
 * group and icon of the app (the budget is 220 kB gzip).
 */
function manualChunks(id: string, { getModuleInfo }: { getModuleInfo: GetModuleInfo }): string | undefined {
  if (!id.includes('/node_modules/')) return undefined
  if (/\/node_modules\/(react|react-dom|react-router|scheduler)\//.test(id)) return 'react'
  if (/\/node_modules\/@tanstack\/(react-query|query-core)\//.test(id)) return 'query'
  // The `radix-ui` umbrella re-exports every primitive as a namespace; the
  // bundler keeps only the namespaces each chunk uses, so the primitives
  // themselves are left to it and land with the routes that use them.
  if (/\/node_modules\/(radix-ui|cmdk|lucide-react)\//.test(id)) {
    return isInitial(id, getModuleInfo) ? 'ui' : undefined
  }
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
