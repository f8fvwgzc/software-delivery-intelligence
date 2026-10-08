import { ServerOffIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import { Table, TableBody, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import type { ServiceHealth } from '../../api/types'
import { ServiceRow } from './service-row'

export { ServiceTableSkeleton } from './skeleton'

const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'

/** One row per backing service: health dot, latency and the gRPC endpoint. */
export function ServiceTable({ services }: { services: ServiceHealth[] }) {
  if (services.length === 0) {
    return (
      <EmptyState
        icon={ServerOffIcon}
        title="No services reported"
        description="The gateway answered but listed no backing services. Check its KNOWLEDGE_GRPC_URL, INTELLIGENCE_GRPC_URL and EXECUTION_GRPC_URL settings."
      />
    )
  }

  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader>
          <TableRow className="bg-muted/30 hover:bg-muted/30">
            <TableHead className={HEAD_CLASS}>Service</TableHead>
            <TableHead className={HEAD_CLASS}>Status</TableHead>
            <TableHead className={`${HEAD_CLASS} text-right`}>Latency</TableHead>
            <TableHead className={HEAD_CLASS}>Endpoint</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {services.map((service) => (
            <ServiceRow key={service.name} service={service} />
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
