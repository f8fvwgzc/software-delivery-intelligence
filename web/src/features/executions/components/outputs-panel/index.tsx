import { ExternalLinkIcon } from 'lucide-react'

import { KeyValue } from '@/components/shared/data/key-value'

import { OUTPUT_LABEL } from '../../lib/labels'

interface OutputsPanelProps {
  /** `Execution.outputs`: `pull_request_url`, `image`, `container_url`, `workdir`, … */
  outputs: Record<string, string>
}

/** Keys the run produced in a fixed, meaningful order; anything else follows alphabetically. */
const KNOWN_ORDER = ['pull_request_url', 'container_url', 'image', 'workdir']

function isLink(key: string, value: string) {
  return key.endsWith('_url') || /^https?:\/\//.test(value)
}

/** What the run produced. Renders nothing while `outputs` is empty. */
export function OutputsPanel({ outputs }: OutputsPanelProps) {
  const keys = Object.keys(outputs)
  if (keys.length === 0) return null

  const ordered = keys.toSorted((a, b) => {
    const ia = KNOWN_ORDER.indexOf(a)
    const ib = KNOWN_ORDER.indexOf(b)
    if (ia !== -1 || ib !== -1) return (ia === -1 ? KNOWN_ORDER.length : ia) - (ib === -1 ? KNOWN_ORDER.length : ib)
    return a.localeCompare(b)
  })

  return (
    <section className="flex flex-col overflow-hidden rounded-lg ring-1 ring-border" aria-label="Outputs">
      <header className="flex h-10 shrink-0 items-center gap-2 border-b px-3">
        <h2 className="text-sm font-medium">Outputs</h2>
        <span className="text-2xs tabular-nums text-muted-foreground">{ordered.length}</span>
      </header>
      <KeyValue
        className="px-3 py-2"
        items={ordered.map((key) => ({
          label: OUTPUT_LABEL[key] ?? key,
          value: isLink(key, outputs[key]) ? (
            <a
              href={outputs[key]}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex max-w-full items-center gap-1 font-mono text-xs text-foreground underline-offset-4 hover:underline"
            >
              <span className="truncate">{outputs[key].replace(/^https?:\/\//, '')}</span>
              <ExternalLinkIcon className="size-3 shrink-0 text-muted-foreground" aria-label="opens in a new tab" />
            </a>
          ) : (
            <span className="font-mono text-xs" title={outputs[key]}>
              {outputs[key]}
            </span>
          ),
        }))}
      />
    </section>
  )
}
