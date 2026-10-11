import { useMutation, useQueryClient } from '@tanstack/react-query'

import { routes } from '@/app/router/routes'
import { api } from '@/lib/api/client'
import { workspaceStore } from '@/lib/workspace-store'

/**
 * `POST /auth/logout`, then a full navigation to the login page: a reload is
 * the one sure way to drop every cache, stream and in-flight request of the
 * old session. Browser state is cleared even when the gateway refuses, since
 * the cookie may already be gone.
 */
export function useLogout() {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: async () => {
      await api.POST('/api/v1/auth/logout')
    },
    meta: { silent: true },
    onSettled: () => {
      workspaceStore.set(null)
      queryClient.clear()
      window.location.assign(routes.login())
    },
  })
}

/** Resolves the install URL on demand and sends the browser there. */
export function useInstallApp() {
  return useMutation({
    mutationFn: async () => {
      const { data } = await api.GET('/api/v1/github/app/install-url')
      return data!
    },
    onSuccess: ({ url }) => {
      window.location.assign(url)
    },
  })
}
