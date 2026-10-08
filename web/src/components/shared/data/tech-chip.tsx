import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'

interface TechChipProps {
  children: string
  className?: string
}

/** Small neutral chip for a technology name. */
export function TechChip({ children, className }: TechChipProps) {
  return (
    <Badge variant="secondary" className={cn('h-5 px-1.5 text-2xs font-normal', className)}>
      {children}
    </Badge>
  )
}

/** A wrapping row of chips, with a "+N" overflow. */
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
