import { PlusIcon } from 'lucide-react'
import { useState } from 'react'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import type { Cluster } from '../api/types'
import { ClusterEditor } from '../components/cluster-editor'
import { ClustersList } from '../components/clusters-list'
import { DeleteClusterDialog } from '../components/delete-cluster-dialog'

/** /workspace/clusters — where GitOps applications deploy (CONTRACT.md §24.2). Changing them is an admin's. */
export default function ClustersPage() {
  const { currentWorkspace } = useAuth()
  const canEdit = hasRole(currentWorkspace.myRole, 'admin')
  // Each dialog keeps its subject while it closes; `open` alone says whether it shows.
  const [editorOpen, setEditorOpen] = useState(false)
  const [editing, setEditing] = useState<Cluster | undefined>(undefined)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleting, setDeleting] = useState<Cluster | undefined>(undefined)

  const openEditor = (cluster?: Cluster) => {
    setEditing(cluster)
    setEditorOpen(true)
  }
  const openDelete = (cluster: Cluster) => {
    setDeleting(cluster)
    setDeleteOpen(true)
  }
  const addButton = (variant: 'default' | 'outline') => (
    <Button variant={variant} size="sm" onClick={() => openEditor()}>
      <PlusIcon data-icon="inline-start" />
      Add cluster
    </Button>
  )

  return (
    <Page>
      <PageHeader
        title="Clusters"
        description="Where applications deploy. A cluster is a name and the workspace secret that holds its kubeconfig; SDI reads the kubeconfig in memory only, and never shows or returns it."
        actions={canEdit ? addButton('default') : null}
      />
      <Panel title="Clusters" description="Each is reached when it is saved; the status is the answer of its last check." padding="none" skeletonRows={2}>
        <ClustersList canEdit={canEdit} onEdit={openEditor} onDelete={openDelete} emptyAction={canEdit ? addButton('outline') : null} />
      </Panel>

      <ClusterEditor cluster={editing} open={editorOpen} onOpenChange={setEditorOpen} />
      <DeleteClusterDialog cluster={deleting} open={deleteOpen} onOpenChange={setDeleteOpen} />
    </Page>
  )
}
