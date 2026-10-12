import { useQuery } from '@tanstack/react-query'

import type { SecretNameFieldProps } from '@/components/shared/form'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import { useSetSecret } from '../api/mutations'
import { cdQueries } from '../api/queries'

type SecretFieldData = Pick<SecretNameFieldProps, 'secrets' | 'loading' | 'unavailable' | 'onAdd'>

/**
 * What a `SecretNameField` needs: the workspace's secret names and a way to
 * store a new one. Both are admin only (CONTRACT.md §22), and the editors
 * that use this are too; anyone else gets a field that takes a typed name
 * and never asks the API. The names are read only while `enabled` (an open
 * editor that has a secret to pick), never on a page's first paint.
 */
export function useSecretField(enabled: boolean): SecretFieldData {
  const { currentWorkspace } = useAuth()
  const isAdmin = hasRole(currentWorkspace.myRole, 'admin')
  const wanted = enabled && isAdmin
  const query = useQuery({ ...cdQueries.secrets(currentWorkspace.id), enabled: wanted })
  const setSecret = useSetSecret(currentWorkspace.id)

  return {
    secrets: query.data ?? [],
    loading: wanted && query.isPending,
    unavailable: !isAdmin || query.isError,
    onAdd: isAdmin ? (secret) => setSecret.mutateAsync(secret) : undefined,
  }
}
