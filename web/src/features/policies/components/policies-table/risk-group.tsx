import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'

import { RISK } from '../../lib/labels'
import { PolicyRow } from './policy-row'

interface RiskGroupProps {
  risk: components['schemas']['RiskLevel']
  policies: components['schemas']['Policy'][]
}

/** One risk tier: heading with the dot, then its policies. */
export function RiskGroup({ risk, policies }: RiskGroupProps) {
  const meta = RISK[risk]

  return (
    <section className="flex flex-col gap-2" aria-labelledby={`risk-${risk}`}>
      <div className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5">
        <h3 id={`risk-${risk}`} className="inline-flex items-center gap-2 text-sm font-medium">
          <span aria-hidden className={`inline-block size-1.5 rounded-full ${meta.dot}`} />
          {meta.label}
          <span className="text-xs font-normal tabular-nums text-muted-foreground">{policies.length}</span>
        </h3>
        <p className="text-xs text-muted-foreground">{meta.description}</p>
      </div>
      <div className="overflow-hidden rounded-lg ring-1 ring-border">
        <Table>
          <TableHeader className="bg-muted/40">
            <TableRow className="hover:bg-transparent">
              <TableHead className="h-9 text-xs">Action</TableHead>
              <TableHead className="h-9 text-xs">Key</TableHead>
              <TableHead className="h-9 w-44 text-xs">Decision</TableHead>
              <TableHead className="h-9 text-right text-xs">Changed</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {policies.map((policy) => (
              <PolicyRow key={policy.actionType} policy={policy} />
            ))}
          </TableBody>
        </Table>
      </div>
    </section>
  )
}
