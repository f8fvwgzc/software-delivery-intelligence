/*
 * The built app without a stack: `web/dist` (from `pnpm build`) served to
 * the page by `page.route`, and the session and the shell's own requests
 * fulfilled in the browser. For specs whose every API answer is typed and
 * fulfilled by the case anyway, so they run where no gateway and no dev
 * server answer. Nothing is listened on: every request the page makes ends
 * here, and an API path a case did not fulfil answers 404, never the network.
 */
import { existsSync } from 'node:fs'
import { readFile } from 'node:fs/promises'
import path from 'node:path'

import type { Page, Route } from '@playwright/test'

import type { components } from '../../src/lib/api/schema'

type Schemas = components['schemas']

const DIST = path.resolve(import.meta.dirname, '../../dist')

const TYPES: Record<string, string> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.ico': 'image/x-icon',
  '.json': 'application/json',
  '.woff2': 'font/woff2',
  '.woff': 'font/woff',
  '.md': 'text/markdown; charset=utf-8',
  '.txt': 'text/plain; charset=utf-8',
}

/** True when `pnpm build` left an app to serve. */
export function hasBuiltApp(): boolean {
  return existsSync(path.join(DIST, 'index.html'))
}

const json = (body: unknown, status = 200) => ({ status, contentType: 'application/json', body: JSON.stringify(body) })

async function serveFile(route: Route, pathname: string) {
  // Only files under dist; anything else is the SPA's index.html.
  const relative = path.normalize(decodeURIComponent(pathname)).replace(/^([/\\])+/, '')
  const file = path.join(DIST, relative)
  const inside = file.startsWith(DIST + path.sep)
  const target = inside && relative && existsSync(file) && path.extname(file) ? file : path.join(DIST, 'index.html')
  const body = await readFile(target)
  await route.fulfill({ status: 200, body, contentType: TYPES[path.extname(target)] ?? 'application/octet-stream' })
}

export const OFFLINE_WORKSPACE_ID = 'e2e-offline-workspace'
export const OFFLINE_USER_ID = '00000000-0000-4000-8000-0000000000e2'

/** The signed-in session the offline shell opens with, at `role`. */
export function offlineMe(role: Schemas['Role']): Schemas['Me'] {
  const now = '2026-10-20T09:00:00Z'
  return {
    currentWorkspaceId: OFFLINE_WORKSPACE_ID,
    user: {
      id: OFFLINE_USER_ID,
      login: 'e2e-operator',
      name: 'E2E Operator',
      avatarUrl: '',
      email: null,
      githubHost: 'github.com',
      githubId: 1,
      createdAt: now,
    },
    workspaces: [
      {
        id: OFFLINE_WORKSPACE_ID,
        slug: 'acme',
        name: 'Acme',
        kind: 'organization',
        avatarUrl: '',
        githubHost: 'github.com',
        installationId: 1,
        memberCount: 3,
        projectCount: 1,
        myRole: role,
        repositorySelection: 'selected',
        createdAt: now,
      },
    ],
  }
}

/**
 * Serves the built app to `page` and answers what the shell asks for on
 * every page: the session at `role`, the system status, the pending
 * approvals. The event stream is left pending, so the app runs on its
 * polling fallback exactly as it does while the stream is down. Register it
 * before the case's own routes: routes registered later win.
 */
export async function serveBuiltApp(page: Page, { role }: { role: Schemas['Role'] }) {
  await page.route('**/*', async (route) => {
    const url = new URL(route.request().url())
    if (url.pathname.startsWith('/api/')) {
      return route.fulfill(json({ code: 'not_found', message: `${url.pathname} is not fulfilled by this spec` } satisfies Schemas['ErrorBody'], 404))
    }
    return serveFile(route, url.pathname)
  })
  await page.route((url) => url.pathname === '/api/v1/events/stream', () => {
    // Never answered: the stream stays "connecting" and the queries keep their fallback polling.
  })
  await page.route((url) => url.pathname === '/api/v1/auth/me', (route) => route.fulfill(json(offlineMe(role))))
  await page.route((url) => url.pathname === '/api/v1/system/status', (route) =>
    route.fulfill(
      json({
        status: 'ok',
        version: 'e2e',
        startedAt: '2026-10-20T08:00:00Z',
        uptimeSeconds: 3600,
        services: [],
      } satisfies Schemas['SystemStatus']),
    ),
  )
  await page.route((url) => url.pathname === '/api/v1/approvals', (route) => route.fulfill(json([] satisfies Schemas['Approval'][])))
}
