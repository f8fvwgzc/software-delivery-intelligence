import { Card } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

const CELL_WIDTHS = ['w-4', 'w-20', 'w-10', 'w-28', 'w-20', 'w-48', 'w-16', 'w-20', 'w-12', 'w-4'] as const

function StatsSkeleton() {
  return (
    <div className="grid gap-3 sm:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)_minmax(0,1fr)]">
      <Card size="sm" className="gap-2 px-3 py-3">
        <Skeleton className="h-3 w-20" />
        <Skeleton className="h-6 w-16" />
        <Skeleton className="h-1.5 w-full rounded-full" />
        <Skeleton className="h-3 w-40" />
      </Card>
      <Card size="sm" className="gap-1.5 px-3 py-3">
        <Skeleton className="h-3 w-24" />
        <Skeleton className="h-6 w-14" />
        <Skeleton className="h-3 w-20" />
      </Card>
      <Card size="sm" className="gap-1.5 px-3 py-3">
        <Skeleton className="h-3 w-12" />
        <Skeleton className="h-6 w-8" />
        <Skeleton className="h-3 w-24" />
      </Card>
    </div>
  )
}

/** Stats strip plus a table with the real column structure. */
export function RunsSkeleton({ rows = 8 }: { rows?: number }) {
  return (
    <div className="flex flex-col gap-3" aria-hidden>
      <StatsSkeleton />
      <div className="overflow-hidden rounded-lg ring-1 ring-border">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              {CELL_WIDTHS.map((width, index) => (
                <TableHead key={index} className="px-3">
                  <Skeleton className={`h-3 ${width}`} />
                </TableHead>
              ))}
            </TableRow>
          </TableHeader>
          <TableBody>
            {Array.from({ length: rows }, (_, row) => (
              <TableRow key={row} className="hover:bg-transparent">
                {CELL_WIDTHS.map((width, index) => (
                  <TableCell key={index} className="px-3 py-2">
                    <Skeleton className={`h-4 ${width}`} />
                  </TableCell>
                ))}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </div>
  )
}
