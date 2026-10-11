import { routes } from '@/app/router/routes'

const PARAM = 'returnTo'

/** Only a same-origin path survives: `//evil` and absolute URLs fall back to the overview. */
export function sanitizeReturnTo(value: string | null | undefined): string {
  if (!value || !value.startsWith('/') || value.startsWith('//') || value.startsWith('/login')) {
    return routes.overview()
  }
  return value
}

/** The login URL that brings the user back to `path` after GitHub. */
export function loginUrl(path: string) {
  const params = new URLSearchParams({ [PARAM]: sanitizeReturnTo(path) })
  return `${routes.login()}?${params.toString()}`
}

/** The gateway endpoint that starts the OAuth dance (dev mode: a local session). */
export function gatewayLoginUrl(returnTo: string) {
  const params = new URLSearchParams({ [PARAM]: sanitizeReturnTo(returnTo) })
  return `/api/v1/auth/login?${params.toString()}`
}

export function readReturnTo(search: string) {
  return sanitizeReturnTo(new URLSearchParams(search).get(PARAM))
}
