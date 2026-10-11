import { ArrowUpRightIcon, ChevronRightIcon } from 'lucide-react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import { ScoreBar } from '@/components/shared/data/score-bar'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import type { MaturityAxis } from '@/features/projects/api/queries'

const VISIBLE_FINDINGS = 2

interface AxisRowProps {
  axis: MaturityAxis
  patternNames?: ReadonlyMap<string, string>
  projectId: string
}

/** Where a "Propose" link lands: the new-proposal flow, preset to a modernization. */
function proposeTarget(projectId: string, patternId: string) {
  const search = new URLSearchParams({ kind: 'modernization', patternId })
  return { pathname: routes.projectProposals(projectId), search: `?${search}` }
}

const LINK_CLASS =
  'inline-flex items-center gap-1 rounded-sm text-xs text-foreground/80 underline-offset-4 outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring/50'

/** One axis: thin bar, summary, the findings under it (two visible), the patterns that would raise it. */
export function AxisRow({ axis, patternNames, projectId }: AxisRowProps) {
  const visible = axis.findings.slice(0, VISIBLE_FINDINGS)
  const hidden = axis.findings.slice(VISIBLE_FINDINGS)

  return (
    <li className="flex flex-col gap-1.5 border-b border-border/60 py-3 first:pt-0 last:border-0 last:pb-0">
      <ScoreBar value={axis.score / 100} label={axis.label} />
      {axis.summary ? <p className="pl-30 text-xs text-muted-foreground">{axis.summary}</p> : null}

      {axis.findings.length > 0 ? (
        <Collapsible className="group/findings flex flex-col gap-1 pl-30">
          <ul className="flex flex-col gap-0.5 text-xs">
            {visible.map((finding) => (
              <li key={finding} className="text-warning">
                {finding}
              </li>
            ))}
          </ul>
          {hidden.length > 0 ? (
            <>
              <CollapsibleContent className="flex flex-col gap-0.5 text-xs">
                {hidden.map((finding) => (
                  <span key={finding} className="text-warning">
                    {finding}
                  </span>
                ))}
              </CollapsibleContent>
              <CollapsibleTrigger className="inline-flex w-fit items-center gap-1 rounded-sm text-2xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50">
                <ChevronRightIcon className="size-3 transition-transform duration-150 motion-reduce:transition-none group-data-[state=open]/findings:rotate-90" />
                <span className="group-data-[state=open]/findings:hidden">Show {hidden.length} more</span>
                <span className="hidden group-data-[state=open]/findings:inline">Show fewer</span>
              </CollapsibleTrigger>
            </>
          ) : null}
        </Collapsible>
      ) : null}

      {axis.recommendedPatterns.length > 0 ? (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 pl-30">
          {axis.recommendedPatterns.map((patternId) => {
            const name = patternNames?.get(patternId)
            return (
              <Link key={patternId} to={proposeTarget(projectId, patternId)} className={LINK_CLASS}>
                Propose: {name ?? <span className="font-mono">{patternId}</span>}
                <ArrowUpRightIcon className="size-3" aria-hidden />
              </Link>
            )
          })}
        </div>
      ) : null}
    </li>
  )
}
