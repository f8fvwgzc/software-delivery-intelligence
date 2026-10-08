import { TableCell, TableRow } from '@/components/ui/table'

import type { ServiceHealth } from '../../api/types'
import { formatLatency } from '../../lib/format'
import { SERVICE_STATUS, serviceName } from '../../lib/labels'
import { HealthDot } from './health-dot'

export function ServiceRow({ service }: { service: ServiceHealth }) {
  const status = SERVICE_STATUS[service.status]

  return (
    <TableRow>
      <TableCell className="px-3 py-2 font-medium">{serviceName(service.name)}</TableCell>
      <TableCell className="px-3 py-2">
        <div className="flex flex-col gap-0.5">
          <span className="flex items-center gap-2">
            <HealthDot tone={status.tone} />
            {status.label}
          </span>
          {service.detail ? (
            <span className="truncate text-xs text-muted-foreground" title={service.detail}>
              {service.detail}
            </span>
          ) : null}
        </div>
      </TableCell>
      <TableCell className="px-3 py-2 text-right font-mono text-xs tabular-nums text-muted-foreground">
        {service.status === 'unreachable' ? '—' : formatLatency(service.latencyMs)}
      </TableCell>
      <TableCell className="px-3 py-2 font-mono text-xs text-muted-foreground">{service.url}</TableCell>
    </TableRow>
  )
}
