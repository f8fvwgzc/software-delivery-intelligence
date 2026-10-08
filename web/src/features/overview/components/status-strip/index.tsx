import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import type { StatusTone } from '@/components/shared/data'
import type { SystemStatus } from '@/features/system/api/types'
import { formatLatency, formatUptime } from '@/features/system/lib/format'
import { overallHealth, SERVICE_STATUS, serviceName } from '@/features/system/lib/labels'
import { cn } from '@/lib/utils'

import { DOT_CLASS } from '../../lib/labels'

export { StatusStripSkeleton } from './skeleton'

function Dot({ tone }: { tone: StatusTone }) {
  return <span aria-hidden className={cn('size-1.5 shrink-0 rounded-full', DOT_CLASS[tone])} />
}

/** Service health at a glance; the whole strip links to the system page. */
export function StatusStrip({ status }: { status: SystemStatus }) {
  const overall = overallHealth(status)

  return (
    <Link
      to={routes.system()}
      className="flex h-full min-h-24 flex-col justify-between gap-2 rounded-lg px-3 py-2.5 ring-1 ring-border transition-colors hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
    >
      <span className="flex items-center justify-between gap-2 text-xs">
        <span className="text-muted-foreground">Services</span>
        <span className="flex items-center gap-1.5">
          <Dot tone={overall.tone} />
          {overall.label}
        </span>
      </span>

      <ul className="flex flex-col gap-1">
        {status.services.map((service) => (
          <li key={service.name} className="flex items-center gap-2 text-xs">
            <Dot tone={SERVICE_STATUS[service.status].tone} />
            <span className="truncate">{serviceName(service.name)}</span>
            <span className="ml-auto font-mono tabular-nums text-muted-foreground">
              {service.status === 'unreachable' ? '—' : formatLatency(service.latencyMs)}
            </span>
          </li>
        ))}
      </ul>

      <span className="truncate font-mono text-2xs tabular-nums text-muted-foreground">
        Gateway {status.version ? `v${status.version}` : ''} · up {formatUptime(status.uptimeSeconds)}
      </span>
    </Link>
  )
}
