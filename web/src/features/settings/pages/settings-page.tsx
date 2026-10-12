import { useQuery, useSuspenseQuery } from '@tanstack/react-query'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'
import { githubQueries } from '@/features/github/api/queries'

import { settingsQueries } from '../api/queries'
import { EnvironmentBlock } from '../components/environment-block'
import { EnvironmentSkeleton, FormSkeleton } from '../components/form-skeleton'
import { RankingForm } from '../components/ranking-form'
import { ScannerForm } from '../components/scanner-form'

/* Panels: the three share one query; each suspends inside its own boundary. */

function ScannerPanel() {
  const { data } = useSuspenseQuery(settingsQueries.settings())
  return <ScannerForm settings={data.knowledge} />
}

function RankingPanel() {
  const { data } = useSuspenseQuery(settingsQueries.settings())
  return <RankingForm settings={data.intelligence} />
}

function EnvironmentPanel() {
  const { data } = useSuspenseQuery(settingsQueries.settings())
  // The token check is a nicety; the block renders without it.
  const account = useQuery(githubQueries.account())
  return <EnvironmentBlock intelligence={data.intelligence} account={account.data} accountError={account.isError} />
}

export default function SettingsPage() {
  return (
    <Page>
      <PageHeader
        title="Settings"
        description="Scanner limits and ranking defaults. Each section saves on its own; environment values are shown, not edited."
      />
      <div className="grid gap-4 lg:grid-cols-12 lg:items-start">
        <div className="flex flex-col gap-4 lg:col-span-8">
          <Panel title="Scanner" description="How much of a repository one scan reads, and how fast." padding="none" skeleton={<FormSkeleton fields={4} toggle />}>
            <ScannerPanel />
          </Panel>
          <Panel
            title="Ranking"
            description="How proposal options are scored and who reasons about them."
            padding="none"
            skeleton={<FormSkeleton fields={7} columns={4} toggle />}
          >
            <RankingPanel />
          </Panel>
        </div>
        <Panel title="Environment" description="Read from the running services." padding="none" skeleton={<EnvironmentSkeleton />} className="lg:col-span-4">
          <EnvironmentPanel />
        </Panel>
      </div>
    </Page>
  )
}
