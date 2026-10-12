import { useQueryClient } from '@tanstack/react-query'
import { RefreshCwIcon } from 'lucide-react'
import { useState, useTransition } from 'react'
import { useParams } from 'react-router'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'
import { Button } from '@/components/ui/button'
import { ToggleGroup, ToggleGroupItem } from '@/components/ui/toggle-group'

import { pullRequestKeys, type PullRequestFilter } from '../api/keys'
import { pullRequestQueries } from '../api/queries'
import { PullRequestList } from '../components/pull-request-list'

const FILTERS: { value: PullRequestFilter; label: string }[] = [
  { value: 'open', label: 'Open' },
  { value: 'closed', label: 'Closed' },
  { value: 'all', label: 'All' },
]

/** /projects/:projectId/pull-requests — what GitHub has open, and the review of each. */
export default function PullRequestsPage() {
  const { projectId = '' } = useParams()
  const queryClient = useQueryClient()
  const [state, setState] = useState<PullRequestFilter>('open')
  const [isPending, startTransition] = useTransition()
  const [refreshing, setRefreshing] = useState(false)

  const refresh = async () => {
    setRefreshing(true)
    try {
      // `refresh` re-reads GitHub; staleTime 0 so the fetch really happens.
      await queryClient.fetchQuery({ ...pullRequestQueries.list(projectId, state, true), staleTime: 0 })
    } finally {
      setRefreshing(false)
      queryClient.invalidateQueries({ queryKey: pullRequestKeys.all })
    }
  }

  return (
    <Page>
      <PageHeader
        title="Pull requests"
        description="Review a pull request the way the platform proposes: evidence first, you decide, then it posts."
        actions={
          <Button variant="outline" size="sm" onClick={refresh} loading={refreshing}>
            <RefreshCwIcon data-icon="inline-start" />
            Read from GitHub
          </Button>
        }
      />
      <Panel
        title="Pull requests"
        padding="none"
        actions={
          <ToggleGroup
            type="single"
            size="sm"
            variant="outline"
            value={state}
            onValueChange={(value) => {
              if (value) startTransition(() => setState(value as PullRequestFilter))
            }}
            aria-label="Pull request state"
            aria-busy={isPending || undefined}
          >
            {FILTERS.map((filter) => (
              <ToggleGroupItem key={filter.value} value={filter.value} className="h-6 px-2 text-xs">
                {filter.label}
              </ToggleGroupItem>
            ))}
          </ToggleGroup>
        }
      >
        <PullRequestList projectId={projectId} state={state} />
      </Panel>
    </Page>
  )
}
