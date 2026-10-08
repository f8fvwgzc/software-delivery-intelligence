import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'

interface ActionTypeChipProps {
  children: string
  className?: string
}

/** Policy key such as `container.run`, always mono. */
export function ActionTypeChip({ children, className }: ActionTypeChipProps) {
  return (
    <Badge variant="secondary" className={cn('h-5 px-1.5 font-mono text-2xs font-normal', className)}>
      {children}
    </Badge>
  )
}
