import { ShieldOffIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import { ErrorBoundary, type FallbackProps } from 'react-error-boundary'

import { EmptyState, QueryBoundary } from '@/components/shared/feedback'
import { ApiError } from '@/lib/api/errors'

/** 403 has fixed copy (CONTRACT.md §15); everything else is the shell's business. */
function SecretFallback({ error }: FallbackProps) {
  if (error instanceof ApiError && error.status === 403) {
    return (
      <EmptyState
        icon={ShieldOffIcon}
        title="Secret-looking files are never displayed"
        description="This path matches a secret pattern (.env, *.pem, *.key, id_rsa…). The platform never shows its content, even when the repository contains it."
        className="m-4"
      />
    )
  }
  throw error
}

interface FileBoundaryProps {
  children: ReactNode
  fallback: ReactNode
}

/** Suspense + error boundary for one file, with the secret refusal handled in place. */
export function FileBoundary({ children, fallback }: FileBoundaryProps) {
  return (
    <QueryBoundary fallback={fallback}>
      <ErrorBoundary FallbackComponent={SecretFallback}>{children}</ErrorBoundary>
    </QueryBoundary>
  )
}
