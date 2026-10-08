import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

const CELL_WIDTHS = ['w-20', 'w-24', 'w-14', 'w-20', 'w-20', 'w-16', 'w-24', 'w-4'] as const

/** Stats line plus a table with the real column structure. */
export function DeploymentsSkeleton({ rows = 5 }: { rows?: number }) {
  return (
    <div className="flex flex-col gap-3" aria-hidden>
      <div className="flex items-center gap-5">
        <Skeleton className="h-3 w-64" />
        <Skeleton className="h-3 w-20" />
        <Skeleton className="h-3 w-16" />
      </div>
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
