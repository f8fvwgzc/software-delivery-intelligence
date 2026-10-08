/** Error body every gateway endpoint returns for a non-2xx response. */
export interface ApiErrorBody {
  code: string
  message: string
}

const TITLES: Record<number, string> = {
  400: 'Invalid request',
  404: 'Not found',
  409: 'Conflict',
  412: 'Not allowed in this state',
  429: 'Too many requests',
  502: 'Service unavailable',
  503: 'Service unavailable',
  504: 'Service timed out',
}

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
    return TITLES[this.status] ?? 'Request failed'
  }

  get isNotFound() {
    return this.status === 404
  }
}
