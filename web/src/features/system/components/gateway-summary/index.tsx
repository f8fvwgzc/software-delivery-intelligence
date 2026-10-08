import { KeyValue, RelativeTime, StatusBadge } from '@/components/shared/data'

import type { SystemStatus } from '../../api/types'
import { formatUptime } from '../../lib/format'
import { gatewayTone } from '../../lib/labels'

/** Version, overall status and uptime of the gateway process. */
export function GatewaySummary({ status }: { status: SystemStatus }) {
  return (
    <KeyValue
      items={[
        { label: 'Version', value: <span className="font-mono text-xs">{status.version || '—'}</span> },
        {
          label: 'Status',
          value: <StatusBadge tone={gatewayTone(status.status)}>{status.status === 'ok' ? 'Ok' : 'Degraded'}</StatusBadge>,
        },
        { label: 'Uptime', value: <span className="tabular-nums">{formatUptime(status.uptimeSeconds)}</span> },
        { label: 'Started', value: <RelativeTime value={status.startedAt} className="font-mono text-xs" /> },
      ]}
    />
  )
}
