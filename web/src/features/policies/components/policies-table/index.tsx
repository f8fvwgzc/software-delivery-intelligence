import { useSuspenseQuery } from '@tanstack/react-query'
import { ScaleIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import type { components } from '@/lib/api/schema'

import { policyQueries } from '../../api/queries'
import { RISK_ORDER } from '../../lib/labels'
import { RiskGroup } from './risk-group'

export { PoliciesTableSkeleton } from './skeleton'

type Policy = components['schemas']['Policy']
type RiskLevel = components['schemas']['RiskLevel']

function groupByRisk(policies: Policy[]) {
  const groups = new Map<RiskLevel, Policy[]>()
  for (const risk of RISK_ORDER) groups.set(risk, [])
  for (const policy of policies) {
    const bucket = groups.get(policy.risk)
    if (bucket) bucket.push(policy)
    else groups.set(policy.risk, [policy])
  }
  return [...groups.entries()].filter(([, items]) => items.length > 0)
}

/** Every policy row from CONTRACT.md §5.2, grouped by risk tier. */
export function PoliciesTable() {
  const { data: policies } = useSuspenseQuery(policyQueries.list())

  if (policies.length === 0) {
    return (
      <EmptyState
        icon={ScaleIcon}
        title="No policies seeded"
        description="The execution service seeds the policy table on start. Check the system page."
      />
    )
  }

  return (
    <div className="flex flex-col gap-6">
      {groupByRisk(policies).map(([risk, items]) => (
        <RiskGroup key={risk} risk={risk} policies={items} />
      ))}
    </div>
  )
}
