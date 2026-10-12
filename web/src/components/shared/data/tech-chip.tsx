import { Tag } from '@/components/system/tag'
import { cn } from '@/lib/utils'

interface TechChipProps {
  children: string
  className?: string
}

/** Small neutral tag for a technology name. */
export function TechChip({ children, className }: TechChipProps) {
  return <Tag className={className}>{children}</Tag>
}

/** A wrapping row of tags, with a "+N" overflow. */
export function TechChips({ items, max = 6, className }: { items: string[]; max?: number; className?: string }) {
  const visible = items.slice(0, max)
  const hidden = items.length - visible.length

  return (
    <div className={cn('flex flex-wrap items-center gap-1', className)}>
      {visible.map((item) => (
        <TechChip key={item}>{item}</TechChip>
      ))}
      {hidden > 0 && <span className="text-2xs text-muted-foreground">+{hidden}</span>}
    </div>
  )
}
