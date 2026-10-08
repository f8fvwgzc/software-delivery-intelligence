import { cn } from '@/lib/utils'

/** 1–5 as filled dots. Position encodes the value, not colour. */
export function ComplexityDots({ value }: { value: number }) {
  const level = Math.max(0, Math.min(5, Math.round(value)))
  return (
    <span className="inline-flex items-center gap-1" role="img" aria-label={`Complexity ${level} of 5`}>
      {Array.from({ length: 5 }, (_, index) => (
        <span
          key={index}
          className={cn('size-1.5 rounded-full', index < level ? 'bg-foreground/70' : 'bg-border')}
        />
      ))}
    </span>
  )
}
