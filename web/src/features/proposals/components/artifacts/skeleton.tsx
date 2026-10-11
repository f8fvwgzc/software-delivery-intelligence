import { Skeleton } from '@/components/ui/skeleton'

/** Mirrors the files panel: a directory tree on the left, a code pane with numbered lines on the right. */
export function ArtifactsSkeleton({ files = 5, lines = 14 }: { files?: number; lines?: number }) {
  return (
    <div className="@container">
    <div className="grid overflow-hidden rounded-lg ring-1 ring-border @2xl:grid-cols-[14rem_minmax(0,1fr)]">
      <div className="flex flex-col gap-3 border-b p-2 @2xl:border-r @2xl:border-b-0">
        <div className="flex h-6 items-center gap-1.5 px-1.5">
          <Skeleton className="size-3.5 rounded-sm" />
          <Skeleton className="h-3 w-10" />
        </div>
        {Array.from({ length: files }, (_, index) => (
          <div key={index} className="flex h-7 items-center gap-2 px-1.5 pl-5">
            <Skeleton className="size-3.5 rounded-sm" />
            <Skeleton className="h-3" style={{ width: `${40 + ((index * 23) % 45)}%` }} />
          </div>
        ))}
      </div>
      <div className="flex min-w-0 flex-col">
        <div className="flex h-10 items-center gap-2 border-b px-3">
          <Skeleton className="size-3.5 rounded-sm" />
          <Skeleton className="h-3 w-48" />
          <Skeleton className="h-4 w-12 rounded-md" />
          <Skeleton className="ml-auto h-6 w-16 rounded-md" />
          <Skeleton className="h-6 w-20 rounded-md" />
        </div>
        <div className="flex flex-col gap-2 bg-muted/20 px-3 py-2.5">
          {Array.from({ length: lines }, (_, index) => (
            <div key={index} className="flex h-3 items-center gap-4">
              <Skeleton className="h-3 w-6" />
              <Skeleton className="h-3" style={{ width: `${20 + ((index * 37) % 60)}%` }} />
            </div>
          ))}
        </div>
        <div className="flex h-7 items-center border-t px-3">
          <Skeleton className="h-3 w-24" />
        </div>
      </div>
    </div>
    </div>
  )
}
