import { createContext, useContext } from 'react'

import type { Me, Workspace } from '../api/types'

export interface AuthContextValue {
  /** The session, or null while signed out. */
  me: Me | null
  /** The workspace every request is scoped to; null while signed out. */
  currentWorkspace: Workspace | null
  /** Switches workspace: remembers it, drops every cached query, reopens the event stream. */
  setWorkspace: (id: string) => void
}

export const AuthContext = createContext<AuthContextValue | null>(null)

/** The session as the provider sees it; null fields while signed out. */
export function useSession(): AuthContextValue {
  const value = useContext(AuthContext)
  if (!value) throw new Error('useSession must be used inside <AuthProvider>')
  return value
}

export interface Auth {
  me: Me
  user: Me['user']
  workspaces: Workspace[]
  currentWorkspace: Workspace
  setWorkspace: (id: string) => void
}

/** The signed-in session; only valid under the route guard. */
export function useAuth(): Auth {
  const { me, currentWorkspace, setWorkspace } = useSession()
  if (!me || !currentWorkspace) throw new Error('useAuth needs a signed-in session')
  return { me, user: me.user, workspaces: me.workspaces, currentWorkspace, setWorkspace }
}
