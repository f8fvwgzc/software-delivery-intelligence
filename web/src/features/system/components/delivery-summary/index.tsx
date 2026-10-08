import { KeyValue } from '@/components/shared/data'

import type { DeliveryStats } from '../../api/types'
import { formatDuration, formatRate } from '../../lib/format'
import { Count } from '../count'

/** Execution counters across every project. */
export function DeliverySummary({ stats }: { stats: DeliveryStats }) {
  return (
    <KeyValue
      items={[
        { label: 'Executions', value: <Count value={stats.executionsTotal} /> },
        { label: 'Active', value: <Count value={stats.executionsActive} /> },
        { label: 'Paused on a failure', value: <Count value={stats.executionsPaused} /> },
        { label: 'Succeeded', value: <Count value={stats.executionsSucceeded} /> },
        { label: 'Failed', value: <Count value={stats.executionsFailed} /> },
        { label: 'Success rate', value: <Count value={stats.successRate} text={formatRate(stats.successRate)} /> },
        {
          label: 'Mean duration',
          value: <Count value={stats.avgDurationSeconds} text={formatDuration(stats.avgDurationSeconds)} />,
        },
        { label: 'Steps executed', value: <Count value={stats.stepsExecuted} /> },
        { label: 'Failures recovered', value: <Count value={stats.failuresRecovered} /> },
        { label: 'Approvals pending', value: <Count value={stats.approvalsPending} /> },
      ]}
    />
  )
}
