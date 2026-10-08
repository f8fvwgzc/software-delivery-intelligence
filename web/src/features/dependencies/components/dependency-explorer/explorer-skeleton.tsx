import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import { CELL_CLASS, HEAD_CLASS } from './columns'

/** Summary bars, Dependabot frame, toolbar and table rows, in the live layout. */
export function DependencyExplorerSkeleton() {
  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <div className="flex flex-col gap-3">
          <Skeleton className="h-6 w-56" />
          <div className="flex flex-col gap-1.5">
            {Array.from({ length: 4 }, (_, index) => (
              <div key={index} className="grid grid-cols-[7rem_minmax(0,1fr)_3rem] items-center gap-3 px-1.5 py-1">
                <Skeleton className="h-3 w-16" />
                <Skeleton className="h-2 rounded-sm" style={{ width: `${100 - index * 22}%` }} />
                <Skeleton className="ml-auto h-3 w-8" />
              </div>
            ))}
          </div>
        </div>
        <div className="flex flex-col gap-3 rounded-lg p-4 ring-1 ring-border">
          <Skeleton className="h-4 w-40" />
          <Skeleton className="h-4 w-full" />
          <Skeleton className="h-4 w-2/3" />
        </div>
      </div>

      <div className="flex flex-col gap-3">
        <div className="flex items-center gap-2">
          <Skeleton className="h-7 w-44" />
          <Skeleton className="h-7 w-64" />
          <Skeleton className="ml-auto h-3 w-10" />
        </div>
        <div className="overflow-hidden rounded-lg ring-1 ring-border">
          <Table>
            <TableHeader>
              <TableRow className="hover:bg-transparent">
                <TableHead className={HEAD_CLASS}>Package</TableHead>
                <TableHead className={HEAD_CLASS}>Ecosystem</TableHead>
                <TableHead className={HEAD_CLASS}>Version</TableHead>
                <TableHead className={HEAD_CLASS}>Scope</TableHead>
                <TableHead className={HEAD_CLASS}>Manifest</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {Array.from({ length: 8 }, (_, index) => (
                <TableRow key={index} className="hover:bg-transparent">
                  <TableCell className={CELL_CLASS.name}>
                    <Skeleton className="h-4 w-36" />
                  </TableCell>
                  <TableCell className={CELL_CLASS.ecosystem}>
                    <Skeleton className="h-3 w-12" />
                  </TableCell>
                  <TableCell className={CELL_CLASS.version}>
                    <Skeleton className="h-3 w-14" />
                  </TableCell>
                  <TableCell className={CELL_CLASS.scope}>
                    <Skeleton className="h-3 w-12" />
                  </TableCell>
                  <TableCell className={CELL_CLASS.manifest}>
                    <Skeleton className="h-3 w-40" />
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </div>
      </div>
    </div>
  )
}
