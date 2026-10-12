import { Frame } from '@/components/system/frame'
import { Skeleton } from '@/components/ui/skeleton'

/** Header line, three service lines, footer line. */
export function StatusStripSkeleton() {
  return (
    <Frame title="Services" padding="none" className="h-full">
      <div className="flex flex-col gap-1.5 px-3 py-2">
        {Array.from({ length: 3 }, (_, index) => (
          <div key={index} className="flex h-4 items-center justify-between">
            <Skeleton className="h-3 w-20" />
            <Skeleton className="h-3 w-10" />
          </div>
        ))}
      </div>
      <div className="mt-auto border-t px-3 py-2">
        <Skeleton className="h-2.5 w-28" />
      </div>
    </Frame>
  )
}
