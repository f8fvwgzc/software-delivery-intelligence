import { ChevronRightIcon } from 'lucide-react'

import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'

import type { UnderstandingSection } from '../../api/queries'

interface SectionItemProps {
  section: UnderstandingSection
  defaultOpen?: boolean
}

/** One collapsible deep-dive section: title and summary visible, bullets on demand. */
export function SectionItem({ section, defaultOpen = false }: SectionItemProps) {
  return (
    <Collapsible defaultOpen={defaultOpen} className="group/section border-b border-border/60 last:border-0">
      <CollapsibleTrigger className="flex w-full items-center gap-2 rounded-sm py-2.5 text-left outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50">
        <ChevronRightIcon className="size-4 shrink-0 text-muted-foreground transition-transform duration-150 motion-reduce:transition-none group-data-[state=open]/section:rotate-90" />
        <span className="shrink-0 text-sm font-medium">{section.title}</span>
        <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">{section.summary}</span>
        <span className="shrink-0 text-2xs tabular-nums text-muted-foreground">{section.items.length}</span>
      </CollapsibleTrigger>
      <CollapsibleContent className="pb-3 pl-6">
        {section.items.length > 0 ? (
          <ul className="flex list-disc flex-col gap-1 pl-4 text-sm text-foreground/90">
            {section.items.map((item, index) => (
              <li key={index}>{item}</li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-muted-foreground">{section.summary || 'No detail recorded.'}</p>
        )}
      </CollapsibleContent>
    </Collapsible>
  )
}
