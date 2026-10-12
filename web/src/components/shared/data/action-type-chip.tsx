import { Tag } from '@/components/system/tag'

interface ActionTypeChipProps {
  children: string
  /** `secondary` (tinted) in execution rails, `outline` in plan previews. */
  variant?: 'secondary' | 'outline'
  className?: string
}

/** Policy key such as `container.run`, always mono. */
export function ActionTypeChip({ children, variant = 'secondary', className }: ActionTypeChipProps) {
  return (
    <Tag mono variant={variant === 'outline' ? 'outline' : 'tint'} className={className}>
      {children}
    </Tag>
  )
}
