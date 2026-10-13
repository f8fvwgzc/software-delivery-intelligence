import { ArrowRightIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { cn } from '@/lib/utils'

interface ExecutionLinkProps {
  executionId: string
  children: ReactNode
  className?: string
}

/**
 * The way to an execution a request started (a fix commit, a merge, turning
 * Dependabot on): its steps, its approval and its outcome are there.
 */
export function ExecutionLink({ executionId, children, className }: ExecutionLinkProps) {
  return (
    <Link
      to={routes.execution(executionId)}
      className={cn(
        'inline-flex items-center gap-1 rounded-sm text-xs text-foreground outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring/50',
        className,
      )}
    >
      {children}
      <ArrowRightIcon className="size-3 shrink-0" aria-hidden />
    </Link>
  )
}
