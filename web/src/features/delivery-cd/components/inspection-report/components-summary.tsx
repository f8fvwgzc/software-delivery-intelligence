import { ChevronDownIcon } from 'lucide-react'
import { useState } from 'react'

import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import { count } from '@/lib/intl'
import { cn } from '@/lib/utils'

import type { InspectedComponent } from '../../api/types'

/** How many it shows when opened: the report keeps up to 2,000. */
const SHOWN = 300

/** What ships, counted per ecosystem, with the full list on demand. */
export function ComponentsSummary({ components }: { components: InspectedComponent[] }) {
  const [open, setOpen] = useState(false)
  const byEcosystem = new Map<string, number>()
  for (const component of components) byEcosystem.set(component.ecosystem, (byEcosystem.get(component.ecosystem) ?? 0) + 1)

  if (components.length === 0) return <p className="text-xs text-muted-foreground">No package database or manifest was found.</p>

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs">
        {[...byEcosystem.entries()].map(([ecosystem, n]) => (
          <span key={ecosystem}>
            <span className="font-medium tabular-nums">{n}</span> <span className="text-muted-foreground">{ecosystem}</span>
          </span>
        ))}
        <CollapsibleTrigger asChild>
          <Button variant="ghost" size="xs" className="ml-auto">
            {open ? 'Hide' : 'Show'} the list
            <ChevronDownIcon className={cn('transition-transform duration-(--motion-fast)', open && 'rotate-180')} />
          </Button>
        </CollapsibleTrigger>
      </div>
      <CollapsibleContent>
        <ul className="mt-2 max-h-72 overflow-y-auto rounded-md text-2xs ring-1 ring-border">
          {components.slice(0, SHOWN).map((c) => (
            <li key={`${c.ecosystem}/${c.name}/${c.version}/${c.path}`} className="flex gap-2 border-b px-2 py-1 last:border-b-0">
              <span className="min-w-0 flex-1 truncate font-mono" title={c.path}>
                {c.name}
              </span>
              <span className="font-mono text-muted-foreground">{c.version}</span>
              <span className="w-24 truncate text-right text-muted-foreground">{c.ecosystem}</span>
            </li>
          ))}
        </ul>
        {components.length > SHOWN ? (
          <p className="mt-1 text-2xs text-muted-foreground">
            First {SHOWN} of {count(components.length, 'component', 'components')}.
          </p>
        ) : null}
      </CollapsibleContent>
    </Collapsible>
  )
}
