import { ArrowRightIcon, ChevronRightIcon } from 'lucide-react'

import { Tag } from '@/components/system/tag'

const LINE_CLASS = 'h-2 w-px flex-1 bg-border lg:h-px lg:w-2'

/**
 * The hairline into a stage from the one before it. When the stages stack
 * it runs down, above the stage; on wide screens it lies in the column gap
 * to the stage's left, level with the frame headers, so every stage keeps
 * the same width. A stage that promotes automatically shows it here, on the
 * step a release takes without anyone pressing a button.
 */
export function StageConnector({ auto }: { auto: boolean }) {
  return (
    <div className="flex h-9 shrink-0 flex-col items-center lg:absolute lg:top-0 lg:right-full lg:h-8 lg:w-16 lg:flex-row">
      <span className={LINE_CLASS} aria-hidden />
      {auto ? (
        <Tag variant="outline" className="mx-0.5 bg-background" title="Promotes here as soon as the previous stage succeeds, still gated by policy">
          <span className="inline-flex items-center gap-1">
            auto
            <ArrowRightIcon className="size-3 rotate-90 lg:rotate-0" aria-hidden />
            <span className="sr-only">promote into the next stage</span>
          </span>
        </Tag>
      ) : (
        <ChevronRightIcon className="size-3.5 shrink-0 rotate-90 text-muted-foreground/70 lg:rotate-0" aria-hidden />
      )}
      <span className={LINE_CLASS} aria-hidden />
    </div>
  )
}
