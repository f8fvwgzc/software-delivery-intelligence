/** Error body every gateway endpoint returns for a non-2xx response. */
export interface ApiErrorBody {
  code: string
  message: string
}

const TITLES: Record<number, string> = {
  400: 'Invalid request',
  401: 'Signed out',
  403: 'Not allowed',
  404: 'Not found',
  409: 'Conflict',
  412: 'Not allowed in this state',
  429: 'Too many requests',
  502: 'Service unavailable',
  503: 'Service unavailable',
  504: 'Service timed out',
}

/** The services phrase a role refusal as `requires the admin role` (crates/common principal.rs). */
const REQUIRED_ROLE = /requires the (\w+) role/i

export class ApiError extends Error {
  readonly status: number
  readonly code: string

  constructor(status: number, body?: Partial<ApiErrorBody>) {
    super(body?.message || `Request failed with status ${status}`)
    this.name = 'ApiError'
    this.status = status
    this.code = body?.code ?? 'unknown'
  }

  get title() {
    const role = this.requiredRole
    if (role) return `You need the ${role} role`
    return TITLES[this.status] ?? 'Request failed'
  }

  get isNotFound() {
    return this.status === 404
  }

  /** The session cookie is missing, expired or revoked. */
  get isUnauthenticated() {
    return this.status === 401
  }

  /** The caller's role in this workspace is too low (CONTRACT.md §18). */
  get isForbidden() {
    return this.status === 403
  }

  /** The role the refused call asked for, when the body names one. */
  get requiredRole(): string | null {
    if (!this.isForbidden) return null
    return REQUIRED_ROLE.exec(this.message)?.[1] ?? null
  }
}

/**
 * `fetch` itself failed: the browser is offline, the gateway is down, DNS
 * failed, or the request was blocked. The gateway never produced a response,
 * so there is no status to read; the copy must say "could not reach".
 */
export function isNetworkError(error: unknown): boolean {
  if (error instanceof ApiError) return false
  if (error instanceof TypeError) return true
  return error instanceof Error && /failed to fetch|networkerror|load failed|network request failed/i.test(error.message)
}

/** `AbortError`: the query was cancelled (route change, unmount), never a failure to show. */
export function isAbortError(error: unknown): boolean {
  return error instanceof DOMException && error.name === 'AbortError'
}
