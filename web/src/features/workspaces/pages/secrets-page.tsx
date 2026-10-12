import { useQuery } from '@tanstack/react-query'
import { LockIcon, PlusIcon } from 'lucide-react'
import { useState } from 'react'

import { Page, PageHeader } from '@/components/shared/page'
import { Frame } from '@/components/system/frame'
import { Panel, PanelEmpty } from '@/components/system/panel'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole, ROLE_LABEL } from '@/features/auth/lib/roles'
import { cdQueries } from '@/features/delivery-cd/api/queries'
import type { SecretInfo } from '@/features/delivery-cd/api/types'

import { DeleteSecretDialog } from '../components/delete-secret-dialog'
import { SecretsList } from '../components/secrets-list'
import { SetSecretDialog } from '../components/set-secret-dialog'

/**
 * /workspace/secrets — credentials by name (CONTRACT.md §22). Reading the
 * names is admin only, so anyone else gets the page frame and a plain
 * explanation, and the list is never asked for.
 */
export default function SecretsPage() {
  const { currentWorkspace } = useAuth()
  const isAdmin = hasRole(currentWorkspace.myRole, 'admin')
  // Each dialog keeps its subject while it closes; `open` alone says whether it shows.
  const [setOpen, setSetOpen] = useState(false)
  const [replacing, setReplacing] = useState<SecretInfo | undefined>(undefined)
  const [deleteOpen, setDeleteOpen] = useState(false)
  const [deleting, setDeleting] = useState('')
  // The list's own query, read again only for the names the dialog warns about.
  const { data: secrets } = useQuery({ ...cdQueries.secrets(currentWorkspace.id), enabled: isAdmin })

  const openSet = (secret?: SecretInfo) => {
    setReplacing(secret)
    setSetOpen(true)
  }
  const openDelete = (name: string) => {
    setDeleting(name)
    setDeleteOpen(true)
  }
  const setButton = (variant: 'default' | 'outline') => (
    <Button variant={variant} size="sm" onClick={() => openSet()}>
      <PlusIcon data-icon="inline-start" />
      Set secret
    </Button>
  )

  return (
    <Page>
      <PageHeader
        title="Secrets"
        description="Credentials that deployers and registries refer to by name. A value is encrypted when it is stored and never shown again: only its name and last four characters ever leave the server."
        actions={isAdmin ? setButton('default') : null}
      />
      {isAdmin ? (
        <Panel title="Workspace secrets" description={`Usable by every project in ${currentWorkspace.name}.`} padding="none" skeletonRows={3}>
          <SecretsList workspaceId={currentWorkspace.id} onReplace={openSet} onDelete={openDelete} emptyAction={setButton('outline')} />
        </Panel>
      ) : (
        <Frame title="Workspace secrets" padding="none">
          <PanelEmpty
            icon={LockIcon}
            title="Visible to admins only"
            description={`Secret names are shown to workspace admins. Your role in ${currentWorkspace.name} is ${ROLE_LABEL[currentWorkspace.myRole].toLowerCase()}; roles come from GitHub.`}
          />
        </Frame>
      )}

      <SetSecretDialog
        workspaceId={currentWorkspace.id}
        secret={replacing}
        taken={secrets?.map((secret) => secret.name) ?? []}
        open={setOpen}
        onOpenChange={setSetOpen}
      />
      <DeleteSecretDialog workspaceId={currentWorkspace.id} name={deleting} open={deleteOpen} onOpenChange={setDeleteOpen} />
    </Page>
  )
}
