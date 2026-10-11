/**
 * The workspace every request is scoped to (CONTRACT.md §18). One module-level
 * value, persisted per browser so a reload lands in the same workspace, read
 * by the API client middleware and the event-stream URL builder, and written
 * only by the auth provider (after `/auth/me`) and the workspace switcher.
 */

const STORAGE_KEY = 'sdi-workspace'

type Listener = () => void

function readStored(): string | null {
  try {
    return window.localStorage.getItem(STORAGE_KEY)
  } catch {
    return null
  }
}

function writeStored(id: string | null) {
  try {
    if (id) window.localStorage.setItem(STORAGE_KEY, id)
    else window.localStorage.removeItem(STORAGE_KEY)
  } catch {
    // Private mode or blocked storage: the in-memory value still works for this tab.
  }
}

let current: string | null = readStored()
const listeners = new Set<Listener>()

export const workspaceStore = {
  get(): string | null {
    return current
  },
  set(id: string | null) {
    if (id === current) return
    current = id
    writeStored(id)
    for (const listener of listeners) listener()
  },
  subscribe(listener: Listener) {
    listeners.add(listener)
    return () => {
      listeners.delete(listener)
    }
  },
}

/** Appends `?workspace=` for endpoints that cannot carry a header (EventSource). */
export function withWorkspaceParam(url: string): string {
  const id = current
  if (!id) return url
  const [path, query = ''] = url.split('?', 2)
  const params = new URLSearchParams(query)
  params.set('workspace', id)
  return `${path}?${params.toString()}`
}
