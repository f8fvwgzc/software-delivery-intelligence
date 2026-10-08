import { Skeleton } from '@/components/ui/skeleton'

import type { Understanding } from '../../api/queries'
import { SectionItem } from './section-item'

/** Architecture paragraph, then the generated sections, first one open. */
export function DeepDive({ understanding }: { understanding: Understanding }) {
  const hasSections = understanding.sections.length > 0

  if (!understanding.architecture && !hasSections) {
    return <p className="text-sm text-muted-foreground">No deep dive was generated for this commit.</p>
  }

  return (
    <div className="flex flex-col gap-3">
      {understanding.architecture ? (
        <p className="max-w-3xl text-sm leading-relaxed whitespace-pre-line text-foreground/90">
          {understanding.architecture}
        </p>
      ) : null}
      {hasSections ? (
        <div className="flex flex-col">
          {understanding.sections.map((section, index) => (
            <SectionItem key={section.key} section={section} defaultOpen={index === 0} />
          ))}
        </div>
      ) : null}
    </div>
  )
}

export function DeepDiveSkeleton() {
  return (
    <div className="flex flex-col gap-3">
      <Skeleton className="h-4 w-2/3" />
      <div className="flex flex-col">
        {Array.from({ length: 4 }, (_, index) => (
          <div key={index} className="flex items-center gap-2 border-b border-border/60 py-2.5 last:border-0">
            <Skeleton className="size-4" />
            <Skeleton className="h-4 w-28" />
            <Skeleton className="h-3 flex-1" />
          </div>
        ))}
      </div>
    </div>
  )
}
