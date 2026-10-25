import { useSuspenseQuery } from '@tanstack/react-query'
import { ServerIcon } from 'lucide-react'
import type { ReactNode } from 'react'

import { PanelEmpty } from '@/components/system/panel'
import { RowList } from '@/components/system/row'

import { gitopsQueries } from '../../api/queries'
import type { Cluster } from '../../api/types'
import { ClusterRow } from './cluster-row'

interface ClustersListProps {
  canEdit: boolean
  onEdit: (cluster: Cluster) => void
  onDelete: (cluster: Cluster) => void
  /** The "Add cluster" trigger, for the empty state. */
  emptyAction?: ReactNode
}

/** The workspace's clusters by name. */
export function ClustersList({ canEdit, onEdit, onDelete, emptyAction }: ClustersListProps) {
  const { data } = useSuspenseQuery(gitopsQueries.clusters())
  const clusters = data.toSorted((a, b) => a.name.localeCompare(b.name))

  if (clusters.length === 0) {
    return (
      <PanelEmpty
        icon={ServerIcon}
        title="No clusters yet"
        description="A cluster is where applications deploy: a name and the workspace secret that holds its kubeconfig. Add one, then declare an application on a project’s Applications tab."
        action={emptyAction}
      />
    )
  }

  return (
    <RowList aria-label="Clusters">
      {clusters.map((cluster) => (
        <li key={cluster.id}>
          <ClusterRow cluster={cluster} canEdit={canEdit} onEdit={onEdit} onDelete={onDelete} />
        </li>
      ))}
    </RowList>
  )
}
