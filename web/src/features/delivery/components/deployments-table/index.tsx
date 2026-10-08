import { ExternalLinkIcon } from 'lucide-react'

import { RelativeTime } from '@/components/shared/data'
import { TechChip } from '@/components/shared/data/tech-chip'
import { Button } from '@/components/ui/button'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import { shortSha } from '../../lib/format'
import { DEPLOYMENT_STATUS, sourceLabel } from '../../lib/labels'
import type { Deployment } from '../../lib/types'
import { handleRowKeyDown } from '../runs-table/row-keyboard'
import { StatusDot } from '../status-dot'

interface DeploymentRowProps {
  deployment: Deployment
}

function DeploymentRow({ deployment }: DeploymentRowProps) {
  const status = DEPLOYMENT_STATUS[deployment.status]

  return (
    <TableRow tabIndex={0} onKeyDown={handleRowKeyDown} className="focus-visible:bg-muted/60 focus-visible:outline-none">
      <TableCell>
        <TechChip>{deployment.environment || 'unknown'}</TechChip>
      </TableCell>
      <TableCell className="max-w-48 truncate font-mono text-xs">{deployment.ref}</TableCell>
      <TableCell className="font-mono text-xs">{shortSha(deployment.commitSha)}</TableCell>
      <TableCell>
        <StatusDot tone={status.tone} label={status.label} live={status.live} />
      </TableCell>
      <TableCell className="font-mono text-xs">
        <RelativeTime value={deployment.createdAt} />
      </TableCell>
      <TableCell className="max-w-32 truncate">{deployment.creator}</TableCell>
      <TableCell className="text-muted-foreground">{sourceLabel(deployment.source)}</TableCell>
      <TableCell className="w-8 pl-0 text-right">
        {deployment.url ? (
          <Button asChild variant="ghost" size="icon-xs" aria-label="Open deployment">
            <a href={deployment.url} target="_blank" rel="noreferrer">
              <ExternalLinkIcon />
            </a>
          </Button>
        ) : null}
      </TableCell>
    </TableRow>
  )
}

interface DeploymentsTableProps {
  deployments: Deployment[]
}

/** Deployments observed at the provider, newest first. */
export function DeploymentsTable({ deployments }: DeploymentsTableProps) {
  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table className="[&_td]:px-3 [&_td]:py-2 [&_th]:px-3 [&_th]:text-xs [&_th]:font-medium [&_th]:text-muted-foreground">
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead>Environment</TableHead>
            <TableHead>Ref</TableHead>
            <TableHead>Commit</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Created</TableHead>
            <TableHead>Creator</TableHead>
            <TableHead>Source</TableHead>
            <TableHead className="w-8">
              <span className="sr-only">Open</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {deployments.map((deployment) => (
            <DeploymentRow key={deployment.id} deployment={deployment} />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}

export { DeploymentStatsStrip } from './deployment-stats'
export { DeploymentsSkeleton } from './skeleton'
