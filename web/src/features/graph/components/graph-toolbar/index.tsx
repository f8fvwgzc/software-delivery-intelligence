import { MaximizeIcon } from 'lucide-react'

import { Tag } from '@/components/system'
import { Button } from '@/components/ui/button'
import { Field, FieldLabel } from '@/components/ui/field'
import { Spinner } from '@/components/ui/spinner'
import { Switch } from '@/components/ui/switch'

import type { NodeKind } from '../../lib/types'
import { KindFilter } from './kind-filter'
import { NodeSearch } from './node-search'

interface GraphToolbarProps {
  kinds: readonly NodeKind[]
  selectedKinds: readonly NodeKind[]
  onKindsChange: (kinds: NodeKind[]) => void
  search: string
  onSearchChange: (value: string) => void
  showHistory: boolean
  onShowHistoryChange: (value: boolean) => void
  /** The history refetch is in flight; the previous graph stays on screen meanwhile. */
  historyPending: boolean
  visibleCount: number
  totalCount: number
  /** Nodes a finished scan added while the map was open. */
  freshCount: number
  onFitView: () => void
}

/** Ghost-weight controls above the canvas. The one solid action is in the page header, never here. */
export function GraphToolbar({
  kinds,
  selectedKinds,
  onKindsChange,
  search,
  onSearchChange,
  showHistory,
  onShowHistoryChange,
  historyPending,
  visibleCount,
  totalCount,
  freshCount,
  onFitView,
}: GraphToolbarProps) {
  return (
    <div className="flex flex-col gap-2 lg:flex-row lg:items-start lg:justify-between">
      <div className="flex min-w-0 flex-col gap-2 sm:flex-row sm:items-start">
        <NodeSearch value={search} onChange={onSearchChange} />
        <KindFilter kinds={kinds} selected={selectedKinds} onChange={onKindsChange} />
      </div>

      <div className="flex shrink-0 flex-wrap items-center gap-3">
        <span className="text-xs tabular-nums text-muted-foreground" aria-live="polite">
          {visibleCount === totalCount ? `${totalCount} nodes` : `${visibleCount} of ${totalCount} nodes`}
        </span>
        {freshCount > 0 ? (
          <Tag tone="success" dot>
            {freshCount} new
          </Tag>
        ) : null}
        <Field orientation="horizontal" className="w-auto gap-2">
          <Switch id="graph-show-history" size="sm" checked={showHistory} onCheckedChange={onShowHistoryChange} />
          <FieldLabel htmlFor="graph-show-history" className="text-xs font-normal text-muted-foreground">
            Show history
          </FieldLabel>
          {historyPending ? <Spinner className="size-3 text-muted-foreground" /> : null}
        </Field>
        <Button variant="outline" size="sm" onClick={onFitView}>
          <MaximizeIcon data-icon="inline-start" />
          Fit view
        </Button>
      </div>
    </div>
  )
}
