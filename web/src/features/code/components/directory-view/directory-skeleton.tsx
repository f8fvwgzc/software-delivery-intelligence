import { Skeleton } from '@/components/ui/skeleton'
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import { CELL_CLASS, HEAD_CLASS } from './entries-table'

const WIDTHS = ['5rem', '7rem', '4rem', '9rem', '6rem', '8rem', '5.5rem', '6.5rem']

/** The listing's header and eight rows, in the live column geometry. */
export function DirectoryViewSkeleton() {
  return (
    <Table>
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead className={HEAD_CLASS} colSpan={2}>
            Name
          </TableHead>
          <TableHead className={`${HEAD_CLASS} text-right`}>Size</TableHead>
        </TableRow>
      </TableHeader>
      <TableBody>
        {WIDTHS.map((width, index) => (
          <TableRow key={index} className="hover:bg-transparent">
            <TableCell className={CELL_CLASS.icon}>
              <Skeleton className="size-4 rounded-sm" />
            </TableCell>
            <TableCell className={CELL_CLASS.name}>
              <Skeleton className="h-4" style={{ width }} />
            </TableCell>
            <TableCell className={CELL_CLASS.size}>
              <Skeleton className="ml-auto h-3 w-10" />
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  )
}
