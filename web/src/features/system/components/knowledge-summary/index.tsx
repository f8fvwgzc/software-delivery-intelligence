import { KeyValue } from '@/components/shared/data'

import type { KnowledgeStats } from '../../api/types'
import { Count } from '../count'

/** Counters of the knowledge store, one line each. */
export function KnowledgeSummary({ stats }: { stats: KnowledgeStats }) {
  return (
    <KeyValue
      items={[
        { label: 'Connected projects', value: <Count value={stats.projects} /> },
        { label: 'Facts', value: <Count value={stats.facts} /> },
        { label: 'Graph nodes', value: <Count value={stats.nodes} /> },
        { label: 'Graph edges', value: <Count value={stats.edges} /> },
        { label: 'Semantic chunks', value: <Count value={stats.chunks} /> },
        { label: 'Patterns', value: <Count value={stats.patterns} /> },
        { label: 'Open questions', value: <Count value={stats.openQuestions} /> },
      ]}
    />
  )
}
