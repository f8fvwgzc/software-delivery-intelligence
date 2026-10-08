import { RelativeTime } from '@/components/shared/data'
import { TableCell, TableRow } from '@/components/ui/table'
import type { components } from '@/lib/api/schema'

import { DecisionSelect } from './decision-select'

interface PolicyRowProps {
  policy: components['schemas']['Policy']
}

export function PolicyRow({ policy }: PolicyRowProps) {
  return (
    <TableRow>
      <TableCell className="max-w-72">
        <div className="flex min-w-0 flex-col gap-0.5">
          <span className="font-medium">{policy.label}</span>
          <span className="truncate text-xs text-muted-foreground">{policy.description}</span>
        </div>
      </TableCell>
      <TableCell className="font-mono text-xs text-muted-foreground">{policy.actionType}</TableCell>
      <TableCell>
        <DecisionSelect policy={policy} />
      </TableCell>
      <TableCell className="text-right text-xs text-muted-foreground">
        <RelativeTime value={policy.updatedAt} fallback="never" className="font-mono tabular-nums" />
      </TableCell>
    </TableRow>
  )
}
