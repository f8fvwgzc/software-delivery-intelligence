import { AlertCircleIcon, RefreshCwIcon } from 'lucide-react'
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
import { ApiError } from '@/lib/api/errors'
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
  const resolvedTitle =
    title ?? (error instanceof ApiError ? error.title : 'Something went wrong')
  const resolvedDescription =
    description ??
    (error instanceof ApiError && error.requiredRole
      ? 'Your role in this workspace is lower. Roles come from GitHub; an admin can narrow them, never widen them.'
      : error instanceof Error
        ? error.message
        : 'An unexpected error occurred.')

  return (
    <Empty
      className={cn(
        compact ? 'gap-3 py-6' : 'rounded-lg border border-destructive/20 bg-destructive/[0.03] py-10',
        className,
      )}
    >
      <EmptyHeader>
        <EmptyMedia variant="icon" className="bg-destructive/10 text-destructive">
          <AlertCircleIcon className="size-4" />
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
