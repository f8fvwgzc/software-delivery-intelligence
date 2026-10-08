import { CheckIcon, MinusIcon } from 'lucide-react'

import { RelativeTime, ScoreBar } from '@/components/shared/data'
import { Badge } from '@/components/ui/badge'
import type { components } from '@/lib/api/schema'

import { PlanRail } from '../plan-rail'

type Pattern = components['schemas']['Pattern']

/** Expanded row: summary, requirements, trade-offs and the plan template. */
export function PatternDetails({ pattern }: { pattern: Pattern }) {
  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
      <div className="flex flex-col gap-4">
        <p className="text-sm">{pattern.summary}</p>

        <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-1.5 text-xs">
          <dt className="text-muted-foreground">Delivery</dt>
          <dd>{pattern.delivery || 'Not specified'}</dd>
          <dt className="text-muted-foreground">Security</dt>
          <dd>
            <ScoreBar value={pattern.securityScore} className="max-w-48" />
          </dd>
          <dt className="text-muted-foreground">Requires</dt>
          <dd className="flex flex-wrap gap-1">
            {pattern.requires.length === 0 ? (
              <span className="text-muted-foreground">Nothing in particular</span>
            ) : (
              pattern.requires.map((requirement) => (
                <Badge key={requirement} variant="outline" className="h-5 px-1.5 font-mono text-2xs font-normal">
                  {requirement}
                </Badge>
              ))
            )}
          </dd>
          <dt className="text-muted-foreground">Last used</dt>
          <dd>
            <RelativeTime value={pattern.lastUsedAt} fallback="Never" className="font-mono" />
          </dd>
          <dt className="text-muted-foreground">Id</dt>
          <dd className="font-mono">{pattern.id}</dd>
        </dl>

        <div className="grid gap-4 sm:grid-cols-2">
          <PointList title="Pros" items={pattern.pros} icon={CheckIcon} iconClass="text-success" />
          <PointList title="Cons" items={pattern.cons} icon={MinusIcon} iconClass="text-warning" />
        </div>
      </div>

      <section className="flex flex-col gap-2">
        <h4 className="text-xs font-medium text-muted-foreground">
          Plan template <span className="tabular-nums">({pattern.plan.length} steps)</span>
        </h4>
        <PlanRail steps={pattern.plan} />
      </section>
    </div>
  )
}

interface PointListProps {
  title: string
  items: string[]
  icon: typeof CheckIcon
  iconClass: string
}

function PointList({ title, items, icon: Icon, iconClass }: PointListProps) {
  return (
    <div className="flex flex-col gap-1.5">
      <h4 className="text-xs font-medium text-muted-foreground">{title}</h4>
      {items.length === 0 ? (
        <p className="text-xs text-muted-foreground">None listed.</p>
      ) : (
        <ul className="flex flex-col gap-1 text-xs">
          {items.map((item) => (
            <li key={item} className="flex gap-2">
              <Icon className={`mt-0.5 size-3 shrink-0 ${iconClass}`} aria-hidden="true" />
              <span>{item}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
