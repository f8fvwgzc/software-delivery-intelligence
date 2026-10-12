import { useQuery } from '@tanstack/react-query'
import { CoinsIcon } from 'lucide-react'

import { Stat } from '@/components/system/stat'
import { Skeleton } from '@/components/ui/skeleton'
import { systemQueries } from '@/features/system/api/queries'
import { formatNumber } from '@/lib/intl'

/** The reasoner's spend today against its cap, from `/intelligence/stats`. Never suspends. */
export function ReasonerUsageStat({ className }: { className?: string }) {
  const stats = useQuery(systemQueries.intelligenceStats())

  if (stats.isPending) return <Skeleton className="h-14 w-40 rounded-lg" />
  if (stats.isError) {
    return <Stat label="Reasoner tokens today" value="—" delta="Could not read the intelligence store" icon={CoinsIcon} framed className={className} />
  }

  const { reasonerTokensToday: today, reasonerDailyTokenCap: cap } = stats.data
  const over = cap > 0 && today >= cap
  return (
    <Stat
      label="Reasoner tokens today"
      value={formatNumber(today)}
      delta={cap > 0 ? `of ${formatNumber(cap)} cap${over ? ' · reached, heuristic until midnight UTC' : ''}` : 'no daily cap'}
      tone={over ? 'warning' : 'default'}
      icon={CoinsIcon}
      framed
      className={className}
    />
  )
}
