import { DownloadIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import type { Workspace } from '@/features/auth/api/types'
import { hasRole } from '@/features/auth/lib/roles'

import { useExportData } from '../../api/mutations'
import { DeleteDialog } from './delete-dialog'
import { PurgeDialog } from './purge-dialog'

interface DataActionsProps {
  workspace: Workspace
}

interface ActionRowProps {
  title: string
  description: string
  children: React.ReactNode
}

function ActionRow({ title, description, children }: ActionRowProps) {
  return (
    <div className="flex flex-col gap-3 py-3 first:pt-0 last:pb-0 sm:flex-row sm:items-center sm:justify-between">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="text-sm">{title}</span>
        <span className="max-w-[60ch] text-xs text-muted-foreground">{description}</span>
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  )
}

/** Export everything, drop the code cache, or delete the workspace: the three §19 guarantees as buttons. */
export function DataActions({ workspace }: DataActionsProps) {
  const isAdmin = hasRole(workspace.myRole, 'admin')
  const isOwner = hasRole(workspace.myRole, 'owner')
  const exportData = useExportData(workspace.id, workspace.slug)

  return (
    <div className="flex flex-col divide-y divide-border/60 rounded-lg p-4 ring-1 ring-border">
      <ActionRow
        title="Export everything"
        description="One JSON document with every row stored for this workspace, across all services; embeddings omitted as derived data. Audited."
      >
        <Button variant="outline" size="sm" loading={exportData.isPending} disabled={!isAdmin} onClick={() => exportData.mutate()}>
          <DownloadIcon data-icon="inline-start" />
          Export
        </Button>
      </ActionRow>
      <ActionRow
        title="Purge code cache"
        description="Deletes cached file contents and repository trees now, whatever the cache mode says."
      >
        <PurgeDialog workspaceId={workspace.id} disabled={!isAdmin} />
      </ActionRow>
      <ActionRow
        title="Delete workspace"
        description="Removes everything in every service and signs you out. Owner only."
      >
        <DeleteDialog workspace={workspace} isOwner={isOwner} />
      </ActionRow>
    </div>
  )
}
