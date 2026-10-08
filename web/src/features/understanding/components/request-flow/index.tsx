import { ArrowRightIcon } from 'lucide-react'

import { Badge } from '@/components/ui/badge'

/** The hops a request takes, left to right. Wraps on narrow screens. */
export function RequestFlow({ hops }: { hops: string[] }) {
  if (hops.length === 0) {
    return <p className="text-sm text-muted-foreground">No request flow was recorded for this commit.</p>
  }

  return (
    <ol className="flex flex-wrap items-center gap-2" aria-label="Request flow">
      {hops.map((hop, index) => (
        <li key={`${hop}-${index}`} className="flex items-center gap-2">
          {index > 0 ? <ArrowRightIcon className="size-3.5 shrink-0 text-muted-foreground" aria-hidden /> : null}
          <Badge variant="outline" className="font-mono">
            {hop}
          </Badge>
        </li>
      ))}
    </ol>
  )
}
