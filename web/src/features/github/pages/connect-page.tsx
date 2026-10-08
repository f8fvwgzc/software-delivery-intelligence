import { usePrefetchQuery, useSuspenseQuery } from '@tanstack/react-query'
import { KeyRoundIcon } from 'lucide-react'
import { useDeferredValue, useState } from 'react'
import { useNavigate } from 'react-router'

import { routes } from '@/app/router/routes'
import { EmptyState, QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader, Section } from '@/components/shared/page'

import { useConnectRepository } from '../api/mutations'
import { githubQueries } from '../api/queries'
import type { GithubRepo } from '../api/types'
import { AccountCard, AccountCardSkeleton } from '../components/account-card'
import { RepoSearch } from '../components/repo-search'
import { RepoTable, RepoTableSkeleton } from '../components/repo-table'
import { TOKEN_ENV_VAR } from '../lib/labels'

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
      onConnect={onConnect}
      onClearQuery={onClearQuery}
    />
  )
}

/** Gate on the account so a missing token explains itself instead of failing the list. */
function RepoListPanel(props: RepoRowsPanelProps) {
  const { data: account } = useSuspenseQuery(githubQueries.account())

  if (!account.tokenConfigured) {
    return (
      <EmptyState
        icon={KeyRoundIcon}
        title="Nothing to list yet"
        description={`Repositories appear here once ${TOKEN_ENV_VAR} is set for the knowledge service.`}
      />
    )
  }

  return (
    <QueryBoundary fallback={<RepoTableSkeleton />}>
      <RepoRowsPanel {...props} />
    </QueryBoundary>
  )
}

export default function ConnectPage() {
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
        description="Pick a repository the token can see. The first scan starts as soon as it is connected."
      />

      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_18rem] lg:items-start">
        <div className="lg:col-start-2 lg:row-start-1">
          <QueryBoundary fallback={<AccountCardSkeleton />}>
            <AccountPanel />
          </QueryBoundary>
        </div>

        <Section
          title="Repositories"
          description="Already connected repositories open their project instead."
          className="lg:col-start-1 lg:row-start-1"
        >
          <RepoSearch value={query} onChange={setQuery} />
          <QueryBoundary fallback={<RepoTableSkeleton />}>
            <RepoListPanel query={deferredQuery} stale={stale} onClearQuery={clearQuery} />
          </QueryBoundary>
        </Section>
      </div>
    </Page>
  )
}
