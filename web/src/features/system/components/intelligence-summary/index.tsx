import { KeyValue } from '@/components/shared/data'

import type { IntelligenceStats } from '../../api/types'
import { Count } from '../count'

/** Counters of the intelligence store. */
export function IntelligenceSummary({ stats }: { stats: IntelligenceStats }) {
  return (
    <KeyValue
      items={[
        { label: 'Proposals', value: <Count value={stats.proposals} /> },
        { label: 'Open', value: <Count value={stats.proposalsOpen} /> },
        { label: 'Accepted', value: <Count value={stats.proposalsAccepted} /> },
        { label: 'Active decisions', value: <Count value={stats.decisionsActive} /> },
        { label: 'Documents', value: <Count value={stats.documents} /> },
        { label: 'Negotiation messages', value: <Count value={stats.messages} /> },
      ]}
    />
  )
}
