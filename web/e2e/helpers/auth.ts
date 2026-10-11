/*
 * Session helpers (CONTRACT.md §18). Every `/api/v1` route needs the
 * `sdi_session` cookie; in `make dev` (`SDI_DEV_LOGIN=1`) `GET /auth/login`
 * creates a local session without GitHub, redirecting through
 * `/auth/callback`, which sets the cookie. A browser context and its
 * `request` share one cookie jar, so signing in through either is enough
 * for both.
 */
import type { APIRequestContext } from '@playwright/test'

import type { components } from '../../src/lib/api/schema'

export type Me = components['schemas']['Me']
export type Workspace = components['schemas']['Workspace']

/** Signs the context in. Resolves false when the gateway is unreachable or dev login is off. */
export async function signInDev(request: APIRequestContext): Promise<boolean> {
  try {
    // Redirects are followed: login → callback (sets the cookie) → `returnTo`.
    const response = await request.get('/api/v1/auth/login?returnTo=/', { maxRedirects: 5, failOnStatusCode: false })
    if (!response.ok()) return false
    return (await getMe(request)) !== null
  } catch {
    return false
  }
}

/** `GET /auth/me`, or null while signed out. */
export async function getMe(request: APIRequestContext): Promise<Me | null> {
  const response = await request.get('/api/v1/auth/me', { failOnStatusCode: false })
  if (response.status() === 401) return null
  if (!response.ok()) throw new Error(`GET /auth/me answered ${response.status()}`)
  return (await response.json()) as Me
}

export async function signOut(request: APIRequestContext) {
  await request.post('/api/v1/auth/logout', { failOnStatusCode: false })
}
