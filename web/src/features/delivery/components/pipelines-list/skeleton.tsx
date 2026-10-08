import { Card, CardContent, CardHeader } from '@/components/ui/card'
import { Skeleton } from '@/components/ui/skeleton'

function PipelineCardSkeleton({ chips }: { chips: number }) {
  return (
    <Card size="sm" aria-hidden>
      <CardHeader className="gap-1.5">
        <Skeleton className="h-4 w-40" />
        <Skeleton className="h-3 w-56" />
      </CardHeader>
      <CardContent className="flex flex-col gap-3">
        <div className="flex items-center gap-1.5">
          <Skeleton className="h-3 w-12" />
          <Skeleton className="h-5 w-12" />
          <Skeleton className="h-5 w-16" />
        </div>
        <div className="flex items-center gap-7">
          {Array.from({ length: chips }, (_, index) => (
            <Skeleton key={index} className="h-6 w-[120px] rounded-md" />
          ))}
        </div>
      </CardContent>
    </Card>
  )
}

/** Two definition blocks, like the real list. */
export function PipelinesSkeleton() {
  return (
    <div className="flex flex-col gap-3">
      <PipelineCardSkeleton chips={3} />
      <PipelineCardSkeleton chips={2} />
    </div>
  )
}
