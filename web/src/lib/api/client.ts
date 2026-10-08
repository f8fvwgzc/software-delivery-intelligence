import createClient, { type Middleware } from 'openapi-fetch'

import { ApiError, type ApiErrorBody } from './errors'
import type { paths } from './schema'

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
    throw new ApiError(response.status, body)
  },
}

/**
 * Typed client generated from the gateway's OpenAPI contract.
 * Paths, params and response bodies are checked at compile time.
 */
export const api = createClient<paths>({ baseUrl: '/' })

api.use(throwOnError)

/**
 * Unwraps an openapi-fetch result. Errors are already thrown by the
 * middleware, so `data` is only missing for empty (204) responses.
 */
export async function unwrap<T>(request: Promise<{ data?: T }>): Promise<T> {
  const { data } = await request
  return data as T
}
