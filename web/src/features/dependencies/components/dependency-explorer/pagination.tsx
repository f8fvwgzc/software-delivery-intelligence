import { ChevronLeftIcon, ChevronRightIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { ButtonGroup } from '@/components/ui/button-group'

interface TablePaginationProps {
  page: number
  pageCount: number
  onPageChange: (page: number) => void
}

/** "Page x of y" with previous/next. Hidden by the caller when there is one page. */
export function TablePagination({ page, pageCount, onPageChange }: TablePaginationProps) {
  return (
    <nav aria-label="Pagination" className="flex items-center justify-between gap-3">
      <span className="text-xs tabular-nums text-muted-foreground">
        Page {page} of {pageCount}
      </span>
      <ButtonGroup>
        <Button
          variant="outline"
          size="icon-sm"
          aria-label="Previous page"
          disabled={page <= 1}
          onClick={() => onPageChange(page - 1)}
        >
          <ChevronLeftIcon />
        </Button>
        <Button
          variant="outline"
          size="icon-sm"
          aria-label="Next page"
          disabled={page >= pageCount}
          onClick={() => onPageChange(page + 1)}
        >
          <ChevronRightIcon />
        </Button>
      </ButtonGroup>
    </nav>
  )
}
