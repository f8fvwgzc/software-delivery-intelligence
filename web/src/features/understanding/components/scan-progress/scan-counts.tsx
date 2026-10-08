import type { Scan } from '@/features/projects/api/queries'

interface CountProps {
  label: string
  value: number
  /** "of N", shown after the value when the total is known. */
  of?: number
}

function Count({ label, value, of }: CountProps) {
  return (
    <div className="flex items-baseline justify-between gap-3 border-b border-border/60 py-1.5 last:border-0 lg:flex-col lg:items-start lg:gap-0 lg:border-0 lg:py-0">
      <dt className="text-xs text-muted-foreground">{label}</dt>
      <dd className="text-sm tabular-nums">
        {value.toLocaleString()}
        {of !== undefined ? <span className="text-muted-foreground"> of {of.toLocaleString()}</span> : null}
      </dd>
    </div>
  )
}

/** What the scan has produced so far. Numbers, not cards. */
export function ScanCounts({ scan }: { scan: Scan }) {
  return (
    <dl className="flex flex-col lg:gap-3">
      <Count label="Files analysed" value={scan.filesAnalyzed} of={scan.filesTotal} />
      <Count label="Facts" value={scan.factsFound} />
      <Count label="Graph nodes" value={scan.nodesFound} />
      <Count label="Graph edges" value={scan.edgesFound} />
    </dl>
  )
}
