import { ChevronRightIcon } from 'lucide-react'
import { useId, useState } from 'react'

import { Tag } from '@/components/system/tag'
import { cn } from '@/lib/utils'

import type { ResourceGroup as Group } from '../../lib/resources'
import { resourceKey } from '../../lib/resources'
import { ResourceRow } from './resource-row'

interface ResourceGroupProps {
  applicationId: string
  group: Group
  /** Open on first render; a large application opens only the kinds that differ or are unwell. */
  defaultOpen: boolean
}

/** The resources of one kind under a header that folds them, with how many of them differ. */
export function ResourceGroup({ applicationId, group, defaultOpen }: ResourceGroupProps) {
  const id = useId()
  const [open, setOpen] = useState(defaultOpen)

  return (
    <section aria-labelledby={`${id}-title`} className="border-b last:border-b-0">
      <h3 id={`${id}-title`} className="m-0">
        <button
          type="button"
          onClick={() => setOpen((current) => !current)}
          aria-expanded={open}
          aria-controls={`${id}-list`}
          className="flex min-h-8 w-full items-center gap-2 bg-muted/30 px-3 py-1 text-left text-xs font-medium outline-none hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset"
        >
          <ChevronRightIcon
            className={cn('size-3.5 shrink-0 text-muted-foreground transition-transform duration-(--motion-fast)', open && 'rotate-90')}
            aria-hidden
          />
          <span>{group.kind}</span>
          <Tag variant="outline" className="tabular-nums">
            {group.resources.length}
          </Tag>
          {group.outOfSync > 0 ? (
            <Tag tone="warning" className="tabular-nums">
              {group.outOfSync} out of sync
            </Tag>
          ) : null}
          {group.unhealthy > 0 ? (
            <Tag tone="destructive" className="tabular-nums">
              {group.unhealthy} unhealthy
            </Tag>
          ) : null}
        </button>
      </h3>
      {open ? (
        <ul id={`${id}-list`} className="flex min-w-0 flex-col divide-y divide-border border-t" aria-label={`${group.kind} resources`}>
          {group.resources.map((resource) => (
            <ResourceRow key={resourceKey(resource)} applicationId={applicationId} resource={resource} />
          ))}
        </ul>
      ) : null}
    </section>
  )
}
