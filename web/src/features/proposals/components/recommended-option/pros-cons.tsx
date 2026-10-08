import { CheckIcon, MinusIcon } from 'lucide-react'

interface ProsConsProps {
  pros: string[]
  cons: string[]
}

/** Two quiet lists side by side; icons carry the polarity, not colour fills. */
export function ProsCons({ pros, cons }: ProsConsProps) {
  return (
    <div className="grid gap-4 sm:grid-cols-2">
      <PointList title="Pros" items={pros} icon={CheckIcon} iconClass="text-success" empty="No advantages listed." />
      <PointList title="Cons" items={cons} icon={MinusIcon} iconClass="text-warning" empty="No drawbacks listed." />
    </div>
  )
}

interface PointListProps {
  title: string
  items: string[]
  icon: typeof CheckIcon
  iconClass: string
  empty: string
}

function PointList({ title, items, icon: Icon, iconClass, empty }: PointListProps) {
  return (
    <div className="flex flex-col gap-1.5">
      <h3 className="text-xs font-medium text-muted-foreground">{title}</h3>
      {items.length === 0 ? (
        <p className="text-xs text-muted-foreground">{empty}</p>
      ) : (
        <ul className="flex flex-col gap-1.5 text-sm">
          {items.map((item) => (
            <li key={item} className="flex gap-2">
              <Icon className={`mt-1 size-3.5 shrink-0 ${iconClass}`} aria-hidden="true" />
              <span>{item}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  )
}
