import { useQuery } from '@tanstack/react-query'

import { RelativeTime } from '@/components/shared/data/relative-time'
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from '@/components/ui/sheet'
import { Skeleton } from '@/components/ui/skeleton'

import { cdQueries } from '../../api/queries'
import type { Release } from '../../api/types'
import { ReportBody } from './report-body'

interface InspectionSheetProps {
  release: Release
  open: boolean
  onOpenChange: (open: boolean) => void
}

/**
 * The last inspection of a release image (CONTRACT.md §22.1). The list
 * carries the summary; the report is read once, when the sheet opens.
 */
export function InspectionSheet({ release, open, onOpenChange }: InspectionSheetProps) {
  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent side="right" className="gap-0 p-0 sm:max-w-2xl">
        <SheetHeader className="shrink-0 border-b pr-12">
          <SheetTitle className="text-sm">Inspection of {release.version}</SheetTitle>
          <SheetDescription className="text-xs">
            What the image ships, read from its layers in the registry
            {release.inspection ? (
              <>
                {', '}
                <RelativeTime value={release.inspection.inspectedAt} />
              </>
            ) : null}
            .
          </SheetDescription>
        </SheetHeader>
        <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain p-4">{open ? <Report releaseId={release.id} /> : null}</div>
      </SheetContent>
    </Sheet>
  )
}

function Report({ releaseId }: { releaseId: string }) {
  const detail = useQuery(cdQueries.release(releaseId))
  if (detail.isPending) {
    return (
      <div className="flex flex-col gap-2" role="status" aria-label="Loading the report">
        <Skeleton className="h-4 w-2/3" />
        <Skeleton className="h-4 w-1/2" />
        <Skeleton className="h-24 w-full" />
      </div>
    )
  }
  const report = detail.data?.inspection?.report
  if (!report) {
    return <p className="text-xs text-muted-foreground">{detail.isError ? 'The report could not be loaded.' : 'This release has no inspection report.'}</p>
  }
  return <ReportBody report={report} />
}
