import { ArrowRightIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { Frame } from '@/components/system/frame'
import { TagDot } from '@/components/system/tag'
import type { SystemStatus } from '@/features/system/api/types'
import { formatLatency, formatUptime } from '@/features/system/lib/format'
import { overallHealth, SERVICE_STATUS, serviceName } from '@/features/system/lib/labels'

export { StatusStripSkeleton } from './skeleton'

/** Service health at a glance; the frame links to the system page. */
export function StatusStrip({ status }: { status: SystemStatus }) {
  const overall = overallHealth(status)

  return (
    <Frame
      title="Services"
      meta={
        <span className="inline-flex items-center gap-1.5 text-2xs text-muted-foreground">
          <TagDot tone={overall.tone} />
          {overall.label}
        </span>
      }
      actions={
        <Link
          to={routes.system()}
          className="inline-flex h-6 items-center gap-1 rounded-md px-1.5 text-xs text-muted-foreground outline-none hover:bg-muted hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          System
          <ArrowRightIcon className="size-3" aria-hidden />
        </Link>
      }
      padding="none"
      className="h-full"
    >
      <ul className="flex flex-col px-3 py-1.5">
        {status.services.map((service) => (
          <li key={service.name} className="flex h-6 items-center gap-2 text-xs">
            <TagDot tone={SERVICE_STATUS[service.status].tone} />
            <span className="truncate">{serviceName(service.name)}</span>
            <span className="ml-auto font-mono tabular-nums text-muted-foreground">
              {service.status === 'unreachable' ? '—' : formatLatency(service.latencyMs)}
            </span>
          </li>
        ))}
      </ul>
      <span className="mt-auto truncate border-t px-3 py-1.5 font-mono text-2xs tabular-nums text-muted-foreground">
        Gateway {status.version ? `v${status.version}` : ''} · up {formatUptime(status.uptimeSeconds)}
      </span>
    </Frame>
  )
}
