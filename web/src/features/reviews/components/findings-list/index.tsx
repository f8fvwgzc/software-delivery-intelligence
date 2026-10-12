import { ShieldCheckIcon } from 'lucide-react'
import { useMemo, useRef, type CSSProperties } from 'react'

import { PanelEmpty } from '@/components/system/panel'
import { Tag } from '@/components/system/tag'
import { useWindowRows } from '@/components/system/virtual-rows'
import { count } from '@/lib/intl'
import { cn } from '@/lib/utils'

import type { Finding, FindingSeverity, Review } from '../../api/types'
import { SEVERITY, SEVERITY_ORDER, canDecide } from '../../lib/labels'
import { FindingItem } from './finding-item'

type Row = { kind: 'header'; severity: FindingSeverity; total: number } | { kind: 'finding'; finding: Finding }

/** A finding with its evidence and suggestion; the real height is measured per row. */
const ESTIMATED_ROW = 120
const HEADER_ROW = 32

/** Findings grouped by severity, worst first, flattened into one list of header and finding rows. */
function flatten(findings: readonly Finding[]): Row[] {
  const groups = new Map<FindingSeverity, Finding[]>()
  for (const finding of findings) {
    const list = groups.get(finding.severity) ?? []
    list.push(finding)
    groups.set(finding.severity, list)
  }
  const rows: Row[] = []
  for (const severity of SEVERITY_ORDER) {
    const items = groups.get(severity)
    if (!items) continue
    rows.push({ kind: 'header', severity, total: items.length })
    for (const finding of items) rows.push({ kind: 'finding', finding })
  }
  return rows
}

function SeverityHeader({ severity, total, style, index, measure }: { severity: FindingSeverity; total: number; style?: CSSProperties; index?: number; measure?: (element: HTMLElement | null) => void }) {
  const meta = SEVERITY[severity]
  return (
    <li
      aria-label={`${meta.label} findings`}
      data-index={index}
      ref={measure}
      style={style}
      className="flex h-8 items-center gap-2 border-y bg-muted/40 px-3 first:border-t-0"
    >
      <Tag tone={meta.tone}>{meta.label}</Tag>
      <span className="text-2xs tabular-nums text-muted-foreground">{count(total, 'finding', 'findings')}</span>
    </li>
  )
}

/**
 * Findings grouped by severity, worst first. A review of a large pull
 * request can carry hundreds of findings; past 200 rows only the ones on
 * screen are rendered (the list scrolls with the page).
 */
export function FindingsList({ review }: { review: Review }) {
  const rows = useMemo(() => flatten(review.findings), [review.findings])
  const listRef = useRef<HTMLOListElement>(null)
  const view = useWindowRows({ count: rows.length, estimateSize: ESTIMATED_ROW, listRef })
  const decidable = canDecide(review.status)

  if (review.findings.length === 0) {
    return (
      <PanelEmpty
        icon={ShieldCheckIcon}
        title={review.status === 'proposed' || review.status === 'published' ? 'No findings' : 'Findings appear after the analyzers run'}
        description={
          review.status === 'proposed' || review.status === 'published'
            ? 'Every analyzer passed over this diff without a note.'
            : 'Secrets, security, dependencies, Docker, CI, IaC, tests, docs and correctness rules run over the diff.'
        }
      />
    )
  }

  return (
    <ol
      ref={listRef}
      aria-label="Findings"
      data-virtual={view.virtual || undefined}
      className={cn('flex flex-col', view.virtual && 'relative')}
      style={view.virtual ? { height: view.totalSize } : undefined}
    >
      {view.items.map((item) => {
        const row = rows[item.index]
        if (!row) return null
        const style: CSSProperties | undefined = view.virtual
          ? { position: 'absolute', top: 0, left: 0, right: 0, transform: `translateY(${item.start - (view.items[0]?.start ?? 0) + view.offsetTop}px)` }
          : undefined
        if (row.kind === 'header') {
          return (
            <SeverityHeader
              key={`header-${row.severity}`}
              severity={row.severity}
              total={row.total}
              style={style ? { ...style, height: HEADER_ROW } : undefined}
              index={item.index}
              measure={view.virtual ? view.measure : undefined}
            />
          )
        }
        return (
          <FindingItem
            key={row.finding.id}
            finding={row.finding}
            reviewId={review.id}
            decidable={decidable}
            style={style}
            index={item.index}
            measure={view.virtual ? view.measure : undefined}
          />
        )
      })}
    </ol>
  )
}
