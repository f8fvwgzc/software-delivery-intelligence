import { useIsFetching, useQueryClient } from '@tanstack/react-query'
import { RefreshCwIcon } from 'lucide-react'

import { Button } from '@/components/ui/button'
import { githubKeys } from '@/features/github/api/keys'

import { systemKeys } from '../../api/keys'

/** Re-runs every health check and counter on the page; spins while any is in flight. */
export function RefreshButton() {
  const queryClient = useQueryClient()
  const fetchingSystem = useIsFetching({ queryKey: systemKeys.all })
  const fetchingAccount = useIsFetching({ queryKey: githubKeys.account() })
  const refreshing = fetchingSystem + fetchingAccount > 0

  const refresh = () => {
    void queryClient.invalidateQueries({ queryKey: systemKeys.all })
    void queryClient.invalidateQueries({ queryKey: githubKeys.account() })
  }

  return (
    <Button variant="outline" size="sm" onClick={refresh} loading={refreshing}>
      <RefreshCwIcon data-icon="inline-start" />
      Refresh
    </Button>
  )
}
