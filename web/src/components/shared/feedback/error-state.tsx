import { AlertCircleIcon, RefreshCwIcon, WifiOffIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { Button } from '@/components/ui/button'
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from '@/components/ui/empty'
import { ApiError, isNetworkError } from '@/lib/api/errors'
import { cn } from '@/lib/utils'

interface ErrorStateProps {
  /** Either pass the error or an explicit title/description. */
  error?: unknown
  title?: ReactNode
  description?: ReactNode
  onRetry?: () => void
  retryLabel?: string
  retrying?: boolean
  /** Inside a frame body: no box of its own, less padding. */
  compact?: boolean
  className?: string
}

const isOffline = () => typeof navigator !== 'undefined' && !navigator.onLine

/** Title and copy for an error, by what the error is. */
export function describeError(error: unknown): { title: string; description: string; offline: boolean } {
  if (error instanceof ApiError) {
    return {
      title: error.title,
      description: error.requiredRole
        ? 'Your role in this workspace is lower. Roles come from GitHub; an admin can narrow them, never widen them.'
        : error.status >= 502 && error.status <= 504
          ? 'The gateway could not reach the service behind it. It usually recovers within a minute; try again.'
          : error.message,
      offline: false,
    }
  }
  if (isNetworkError(error)) {
    const offline = isOffline()
    return {
      title: offline ? 'You are offline' : 'Could not reach the server',
      description: offline
        ? 'This panel loads again as soon as the connection is back.'
        : 'The gateway did not answer. Check the connection, then try again.',
      offline,
    }
  }
  return {
    title: 'Something went wrong',
    description: error instanceof Error ? error.message : 'An unexpected error occurred.',
    offline: false,
  }
}

export function ErrorState({
  error,
  title,
  description,
  onRetry,
  retryLabel = 'Try again',
  retrying = false,
  compact = false,
  className,
}: ErrorStateProps) {
  const described = describeError(error)
  const resolvedTitle = title ?? described.title
  const resolvedDescription = description ?? described.description
  const Icon = described.offline ? WifiOffIcon : AlertCircleIcon

  return (
    <Empty
      role="alert"
      className={cn(
        compact ? 'gap-3 py-6' : 'rounded-lg border border-destructive/20 bg-destructive/[0.03] py-10',
        className,
      )}
    >
      <EmptyHeader>
        <EmptyMedia variant="icon" className="bg-destructive/10 text-destructive">
          <Icon className="size-4" />
        </EmptyMedia>
        <EmptyTitle>{resolvedTitle}</EmptyTitle>
        <EmptyDescription>{resolvedDescription}</EmptyDescription>
      </EmptyHeader>
      {onRetry && (
        <EmptyContent>
          <Button variant="outline" size="sm" onClick={onRetry} loading={retrying}>
            <RefreshCwIcon data-icon="inline-start" />
            {retryLabel}
          </Button>
        </EmptyContent>
      )}
    </Empty>
  )
}
