import { PlusIcon } from 'lucide-react'
import { useState } from 'react'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'
import type { Registry } from '@/features/delivery-cd/api/types'

import { DeleteRegistryDialog } from '../components/delete-registry-dialog'
import { RegistriesList } from '../components/registries-list'
import { RegistryEditor } from '../components/registry-editor'

/** /workspace/registries — where images live (CONTRACT.md §22). Admin only; the gateway refuses everyone else. */
export default function RegistriesPage() {
  const { currentWorkspace } = useAuth()
  const canEdit = hasRole(currentWorkspace.myRole, 'admin')
  // Each dialog keeps its subject while it closes; `open` alone says whether it shows.
  const [editorOpen, setEditorOpen] = useState(false)
  const [editing, setEditing] = useState<Registry | undefined>(undefined)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleting, setDeleting] = useState<Registry | undefined>(undefined)

  const openEditor = (registry?: Registry) => {
    setEditing(registry)
    setEditorOpen(true)
  }
  const openDelete = (registry: Registry) => {
    setDeleting(registry)
    setDeleteOpen(true)
  }
  const addButton = (variant: 'default' | 'outline') => (
    <Button variant={variant} size="sm" onClick={() => openEditor()}>
      <PlusIcon data-icon="inline-start" />
      Add registry
    </Button>
  )

  return (
    <Page>
      <PageHeader
        title="Registries"
        description="Where built images live. Releases are pushed to the default registry unless an environment names another; a deployment verifies the image there before it deploys."
        actions={canEdit ? addButton('default') : null}
      />
      <Panel
        title="Registries"
        description="GitHub Container Registry is the seeded default and needs no credentials."
        padding="none"
        skeletonRows={2}
      >
        <RegistriesList canEdit={canEdit} onEdit={openEditor} onDelete={openDelete} emptyAction={canEdit ? addButton('outline') : null} />
      </Panel>

      <RegistryEditor registry={editing} open={editorOpen} onOpenChange={setEditorOpen} />
      <DeleteRegistryDialog registry={deleting} open={deleteOpen} onOpenChange={setDeleteOpen} />
    </Page>
  )
}
