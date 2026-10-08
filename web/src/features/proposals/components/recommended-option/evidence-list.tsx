import type { components } from '@/lib/api/schema'

import { EVIDENCE_KIND } from '../../lib/labels'

type Evidence = components['schemas']['Evidence']

/** Every recommendation names its evidence. Icon by kind, label, detail. */
export function EvidenceList({ evidence }: { evidence: Evidence[] }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="text-xs font-medium text-muted-foreground">
        Evidence <span className="tabular-nums">({evidence.length})</span>
      </h3>
      {evidence.length === 0 ? (
        <p className="text-xs text-muted-foreground">No evidence was recorded for this option.</p>
      ) : (
        <ul className="flex flex-col divide-y divide-border/60 rounded-md ring-1 ring-border/60">
          {evidence.map((item, index) => {
            const kind = EVIDENCE_KIND[item.kind]
            return (
              <li key={`${item.refId}-${index}`} className="flex items-start gap-2.5 px-3 py-2 text-sm">
                <kind.icon className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-label={kind.label} />
                <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                  <span className="font-medium leading-5">{item.label}</span>
                  {item.detail ? <span className="text-xs text-muted-foreground">{item.detail}</span> : null}
                </div>
                <span className="shrink-0 font-mono text-2xs text-muted-foreground">{item.refId}</span>
              </li>
            )
          })}
        </ul>
      )}
    </section>
  )
}
