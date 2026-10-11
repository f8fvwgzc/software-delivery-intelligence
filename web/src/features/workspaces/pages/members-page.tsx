import { useSuspenseQuery } from '@tanstack/react-query'
import { RefreshCwIcon } from 'lucide-react'

import { RelativeTime } from '@/components/shared/data'
import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader, Section } from '@/components/shared/page'
import { Button } from '@/components/ui/button'
import { useAuth } from '@/features/auth/lib/auth-context'
import { hasRole } from '@/features/auth/lib/roles'

import { useSyncMembers } from '../api/mutations'
import { workspaceQueries } from '../api/queries'
import { MembersTable, MembersTableSkeleton } from '../components/members-table'

const ROLE_RULES =
  'Roles come from GitHub: organisation owners are owners, the sdi-admins team admins, sdi-approvers approvers, ' +
  'other members members, outside collaborators viewers. An admin can narrow a role below what GitHub implies, never widen it.'

function MembersPanel() {
  const { user, currentWorkspace } = useAuth()
  const { data } = useSuspenseQuery(workspaceQueries.members(currentWorkspace.id))

  return (
    <div className="flex flex-col gap-2">
      <MembersTable
        workspaceId={currentWorkspace.id}
        members={data.members}
        currentUserId={user.id}
        canEdit={hasRole(currentWorkspace.myRole, 'admin')}
      />
      <p className="text-xs text-muted-foreground">
        {data.syncedAt ? (
          <>
            Last read from GitHub <RelativeTime value={data.syncedAt} />.
          </>
        ) : (
          'Not read from GitHub yet.'
        )}
      </p>
    </div>
  )
}

export default function MembersPage() {
  const { currentWorkspace } = useAuth()
  const sync = useSyncMembers(currentWorkspace.id)
  const canSync = hasRole(currentWorkspace.myRole, 'admin')

  return (
    <Page>
      <PageHeader
        title="Members"
        description={ROLE_RULES}
        actions={
          <Button variant="outline" size="sm" loading={sync.isPending} disabled={!canSync} onClick={() => sync.mutate()}>
            <RefreshCwIcon data-icon="inline-start" />
            Sync from GitHub
          </Button>
        }
      />
      <Section title={currentWorkspace.name} description="Membership is read from GitHub on sync, on webhooks, and when someone new opens the workspace.">
        <QueryBoundary fallback={<MembersTableSkeleton />}>
          <MembersPanel />
        </QueryBoundary>
      </Section>
    </Page>
  )
}
