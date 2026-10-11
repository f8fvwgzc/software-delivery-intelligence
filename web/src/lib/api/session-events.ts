/**
 * A 401 from any endpoint means the session is gone (expired, revoked, or
 * signed out in another tab). The client middleware reports it here; the auth
 * provider listens and drops the signed-in user, which sends the router to
 * the login page with the current location as `returnTo`.
 */

type Listener = () => void

const listeners = new Set<Listener>()

export function onUnauthenticated(listener: Listener) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

export function emitUnauthenticated() {
  for (const listener of listeners) listener()
}
