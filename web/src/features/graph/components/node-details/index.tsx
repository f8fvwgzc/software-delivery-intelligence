import type { UseQueryResult } from '@tanstack/react-query'

import { KeyValue } from '@/components/shared/data'
import { Separator } from '@/components/ui/separator'

import { NODE_KIND_META } from '../../lib/labels'
import type { GraphNode, ImpactAnalysis } from '../../lib/types'
import { ImpactAnalysisSection } from './impact-analysis'

interface NodeDetailsProps {
  projectId: string
  node: GraphNode
  impact: UseQueryResult<ImpactAnalysis>
  onSelectNode: (id: string) => void
}

/** Identity, provenance and attributes of one node, then what it would affect. */
export function NodeDetails({ projectId, node, impact, onSelectNode }: NodeDetailsProps) {
  const meta = NODE_KIND_META[node.kind]
  const Icon = meta.icon
  const attributes = Object.entries(node.attributes)

  return (
    <div className="flex flex-col gap-4">
      <header className="flex items-start gap-2.5">
        <span className="flex size-7 shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground">
          <Icon className="size-4" />
        </span>
        <div className="flex min-w-0 flex-col gap-0.5">
          <h3 className="text-sm font-medium leading-5 text-balance">{node.label}</h3>
          <div className="flex min-w-0 items-baseline gap-1.5 text-2xs text-muted-foreground">
            <span>{meta.label}</span>
            <span aria-hidden>·</span>
            <span className="truncate font-mono">{node.key}</span>
          </div>
        </div>
      </header>

      {node.description ? <p className="text-xs text-muted-foreground">{node.description}</p> : null}

      <div className="flex flex-col gap-1">
        <span className="text-2xs text-muted-foreground">Source path</span>
        {node.sourcePath ? (
          <code className="rounded-sm bg-muted px-1.5 py-1 font-mono text-xs break-all">{node.sourcePath}</code>
        ) : (
          <span className="text-xs text-muted-foreground">Not tied to a file</span>
        )}
      </div>

      {attributes.length > 0 ? (
        <KeyValue
          items={attributes.map(([label, value]) => ({
            label,
            value: (
              <span className="text-xs" title={value}>
                {value}
              </span>
            ),
          }))}
        />
      ) : null}

      <Separator />

      <ImpactAnalysisSection projectId={projectId} impact={impact} onSelectNode={onSelectNode} />
    </div>
  )
}
