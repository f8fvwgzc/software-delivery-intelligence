import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import { COLUMN_CLASS, HEAD_CLASS } from './columns'

/** Same columns and row height as the real table, so nothing jumps when data lands. */
export function ProjectsTableSkeleton({ rows = 5 }: { rows?: number }) {
  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader>
          <TableRow className="hover:bg-transparent">
            <TableHead className={HEAD_CLASS}>Repository</TableHead>
            <TableHead className={HEAD_CLASS}>Status</TableHead>
            <TableHead className={HEAD_CLASS}>Mode</TableHead>
            <TableHead className={HEAD_CLASS}>Stack</TableHead>
            <TableHead className={HEAD_CLASS}>Maturity</TableHead>
            <TableHead className={`${HEAD_CLASS} text-right`}>Open questions</TableHead>
            <TableHead className={HEAD_CLASS}>Last scanned</TableHead>
            <TableHead className={HEAD_CLASS}>
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {Array.from({ length: rows }, (_, index) => (
            <TableRow key={index} className="hover:bg-transparent">
              <TableCell className={COLUMN_CLASS.name}>
                <Skeleton className="h-4 w-44" />
              </TableCell>
              <TableCell className={COLUMN_CLASS.status}>
                <Skeleton className="h-4 w-16" />
              </TableCell>
              <TableCell className={COLUMN_CLASS.mode}>
                <Skeleton className="h-4 w-20" />
              </TableCell>
              <TableCell className={COLUMN_CLASS.stack}>
                <div className="flex gap-1">
                  <Skeleton className="h-5 w-14" />
                  <Skeleton className="h-5 w-12" />
                  <Skeleton className="h-5 w-16" />
                </div>
              </TableCell>
              <TableCell className={COLUMN_CLASS.maturity}>
                <div className="flex items-center gap-2">
                  <Skeleton className="h-4 w-6" />
                  <Skeleton className="h-2 w-9" />
                  <Skeleton className="h-3 w-14" />
                </div>
              </TableCell>
              <TableCell className={COLUMN_CLASS.questions}>
                <Skeleton className="ml-auto h-4 w-5" />
              </TableCell>
              <TableCell className={COLUMN_CLASS.scanned}>
                <Skeleton className="h-4 w-24" />
              </TableCell>
              <TableCell className={COLUMN_CLASS.actions}>
                <div className="flex justify-end gap-1">
                  <Skeleton className="size-7" />
                  <Skeleton className="size-7" />
                </div>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
