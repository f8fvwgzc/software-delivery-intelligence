import { Badge } from '@/components/ui/badge'

/** OAuth scopes as small mono chips; "repo" is the one that matters. */
export function ScopeList({ scopes }: { scopes: string[] }) {
  if (scopes.length === 0) {
    return <span className="text-xs text-muted-foreground">No scopes reported</span>
  }

  return (
    <div className="flex flex-wrap gap-1">
      {scopes.map((scope) => (
        <Badge key={scope} variant="secondary" className="h-5 px-1.5 font-mono text-2xs font-normal">
          {scope}
        </Badge>
      ))}
    </div>
  )
}
