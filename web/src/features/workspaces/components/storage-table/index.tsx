import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table'

import { STORAGE_ROWS } from '../../lib/labels'

const HEAD_CLASS = 'h-9 px-3 text-xs font-medium text-muted-foreground'
const CELL_CLASS = 'px-3 py-2 align-top text-xs'

/** CONTRACT.md §19 as the product shows it: what, where, for how long, and whether you can turn it off. */
export function StorageTable() {
  return (
    <div className="overflow-hidden rounded-lg ring-1 ring-border">
      <Table>
        <TableHeader>
          <TableRow className="bg-muted/30 hover:bg-muted/30">
            <TableHead className={HEAD_CLASS}>What</TableHead>
            <TableHead className={`${HEAD_CLASS} hidden md:table-cell`}>Where</TableHead>
            <TableHead className={HEAD_CLASS}>Retention</TableHead>
            <TableHead className={HEAD_CLASS}>Can be turned off</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {STORAGE_ROWS.map((row) => (
            <TableRow key={row.what} className="hover:bg-transparent">
              <TableCell className={`${CELL_CLASS} max-w-72 whitespace-normal text-foreground`}>{row.what}</TableCell>
              <TableCell className={`${CELL_CLASS} hidden max-w-64 whitespace-normal font-mono text-muted-foreground md:table-cell`}>
                {row.where}
              </TableCell>
              <TableCell className={`${CELL_CLASS} max-w-64 whitespace-normal text-muted-foreground`}>{row.retention}</TableCell>
              <TableCell className={`${CELL_CLASS} max-w-48 whitespace-normal text-muted-foreground`}>{row.control}</TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
