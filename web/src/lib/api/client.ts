import createClient, { type Middleware } from 'openapi-fetch'

import { workspaceStore } from '@/lib/workspace-store'

import { ApiError, type ApiErrorBody } from './errors'
import type { paths } from './schema'
import { emitUnauthenticated } from './session-events'

/** Header the gateway resolves the workspace from (CONTRACT.md §18). */
export const WORKSPACE_HEADER = 'X-Sdi-Workspace'

/** Session routes resolve the user, not a workspace; a stale header must not confuse them. */
const AUTH_PREFIX = '/api/v1/auth/'

/** Every request carries the current workspace, so no feature has to know about tenancy. */
const workspaceHeader: Middleware = {
  onRequest({ request }) {
    const id = workspaceStore.get()
    if (!id || new URL(request.url).pathname.startsWith(AUTH_PREFIX)) return undefined
    if (!request.headers.has(WORKSPACE_HEADER)) request.headers.set(WORKSPACE_HEADER, id)
    return request
  },
}

/** Turns every non-2xx response into a thrown `ApiError`, so TanStack Query sees failures. */
const throwOnError: Middleware = {
  async onResponse({ response }) {
    if (response.ok) return undefined

    let body: Partial<ApiErrorBody> | undefined
    try {
      body = await response.clone().json()
    } catch {
      body = undefined
    }
    const error = new ApiError(response.status, body)
    // `/auth/me` answers 401 by design while signed out; it is handled by its query.
    if (error.isUnauthenticated && !new URL(response.url).pathname.startsWith(AUTH_PREFIX)) {
      emitUnauthenticated()
    }
    throw error
  },
}

/**
 * Typed client generated from the gateway's OpenAPI contract.
 * Paths, params and response bodies are checked at compile time.
 */
export const api = createClient<paths>({ baseUrl: '/' })

api.use(workspaceHeader)
api.use(throwOnError)

/**
 * Unwraps an openapi-fetch result. Errors are already thrown by the
 * middleware, so `data` is only missing for empty (204) responses.
 */
export async function unwrap<T>(request: Promise<{ data?: T }>): Promise<T> {
  const { data } = await request
  return data as T
}
