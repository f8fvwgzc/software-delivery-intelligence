import { usePrefetchQuery, useSuspenseQuery } from '@tanstack/react-query'
import { KeyRoundIcon } from 'lucide-react'
import { useDeferredValue, useState } from 'react'
import { useNavigate } from 'react-router'

import { routes } from '@/app/router/routes'
import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader } from '@/components/shared/page'
import { Panel, PanelEmpty } from '@/components/system/panel'
import { useAuth } from '@/features/auth/lib/auth-context'

import { useConnectRepository } from '../api/mutations'
import { githubQueries } from '../api/queries'
import type { GithubRepo } from '../api/types'
import { AccountCard, AccountCardSkeleton } from '../components/account-card'
import { InstallationCard } from '../components/installation-card'
import { RepoSearch } from '../components/repo-search'
import { RepoTable, RepoTableSkeleton } from '../components/repo-table'
import { TOKEN_ENV_VAR } from '../lib/labels'

/** Personal: the OAuth account behind the session. Organisation: the installation, no account to show. */
function AccountPanel() {
  const { data } = useSuspenseQuery(githubQueries.account())
  return <AccountCard account={data} />
}

interface RepoRowsPanelProps {
  query: string
  stale: boolean
  onClearQuery: () => void
}

/** The list itself; only mounted once the account confirms a token exists. */
function RepoRowsPanel({ query, stale, onClearQuery }: RepoRowsPanelProps) {
  const navigate = useNavigate()
  const { currentWorkspace } = useAuth()
  const { data: repos } = useSuspenseQuery(githubQueries.repos(query))
  const connect = useConnectRepository()

  const onConnect = (repo: GithubRepo) => {
    connect.mutate(
      { repoFullName: repo.fullName },
      { onSuccess: (project) => navigate(routes.project(project.id)) },
    )
  }

  return (
    <RepoTable
      repos={repos}
      query={query}
      stale={stale}
      connectingFullName={connect.isPending ? connect.variables.repoFullName : null}
      source={currentWorkspace.kind === 'organization' ? 'installation' : 'token'}
      onConnect={onConnect}
      onClearQuery={onClearQuery}
    />
  )
}

/** Gate on the account so a missing token explains itself instead of failing the list. */
function RepoListPanel(props: RepoRowsPanelProps) {
  const { currentWorkspace } = useAuth()
  const { data: account } = useSuspenseQuery(githubQueries.account())

  // An installation always has a token: it is minted per request (CONTRACT.md §18).
  if (!account.tokenConfigured && currentWorkspace.kind !== 'organization') {
    return (
      <PanelEmpty
        icon={KeyRoundIcon}
        title="Nothing to list yet"
        description={`Repositories appear here once ${TOKEN_ENV_VAR} is set for the knowledge service.`}
      />
    )
  }

  return (
    <QueryBoundary compact fallback={<RepoTableSkeleton />}>
      <RepoRowsPanel {...props} />
    </QueryBoundary>
  )
}

export default function ConnectPage() {
  const { currentWorkspace } = useAuth()
  const organisation = currentWorkspace.kind === 'organization'
  const [query, setQuery] = useState('')
  const deferredQuery = useDeferredValue(query.trim())
  const stale = query.trim() !== deferredQuery

  // Account and repositories start together; the repo prefetch is harmless
  // when no token is configured (prefetch failures never surface).
  usePrefetchQuery(githubQueries.account())
  usePrefetchQuery(githubQueries.repos(deferredQuery))

  const clearQuery = () => setQuery('')

  return (
    <Page>
      <PageHeader
        title="Connect repository"
        description={
          organisation
            ? `Pick a repository the SDI app is installed on in ${currentWorkspace.name}. The first scan starts as soon as it is connected.`
            : 'Pick a repository your GitHub account can see. The first scan starts as soon as it is connected.'
        }
      />

      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_18rem] lg:items-start">
        <div className="lg:col-start-2 lg:row-start-1">
          {organisation ? (
            <InstallationCard workspace={currentWorkspace} />
          ) : (
            <QueryBoundary fallback={<AccountCardSkeleton />}>
              <AccountPanel />
            </QueryBoundary>
          )}
        </div>

        <Panel
          title="Repositories"
          description={
            organisation
              ? 'The installation’s selection; change it on GitHub. Connected ones open their project.'
              : 'Already connected repositories open their project instead.'
          }
          actions={<RepoSearch value={query} onChange={setQuery} />}
          padding="none"
          skeleton={<RepoTableSkeleton />}
          className="lg:col-start-1 lg:row-start-1"
        >
          <RepoListPanel query={deferredQuery} stale={stale} onClearQuery={clearQuery} />
        </Panel>
      </div>
    </Page>
  )
}
