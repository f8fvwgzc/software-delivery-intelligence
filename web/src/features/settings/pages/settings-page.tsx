import { useQuery, useSuspenseQuery } from '@tanstack/react-query'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'
import { githubQueries } from '@/features/github/api/queries'
import { systemQueries } from '@/features/system/api/queries'

import { settingsQueries } from '../api/queries'
import { AutomationForm } from '../components/automation-form'
import { EnvironmentBlock } from '../components/environment-block'
import { AutomationSkeleton, EnvironmentSkeleton, FormSkeleton } from '../components/form-skeleton'
import { RankingForm } from '../components/ranking-form'
import { ReasonerUsageStat } from '../components/reasoner-usage'
import { ScannerForm } from '../components/scanner-form'

/* Panels: the four share one query; each suspends inside its own boundary. */

function ScannerPanel() {
  const { data } = useSuspenseQuery(settingsQueries.settings())
  return <ScannerForm settings={data.knowledge} />
}

function RankingPanel() {
  const { data } = useSuspenseQuery(settingsQueries.settings())
  // Today's spend is a nicety under the cap field; the form renders without it.
  const stats = useQuery(systemQueries.intelligenceStats())
  return <RankingForm settings={data.intelligence} tokensToday={stats.data?.reasonerTokensToday} />
}

function AutomationPanel() {
  const { data } = useSuspenseQuery(settingsQueries.settings())
  return <AutomationForm settings={data.intelligence} />
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
        description="Scanner limits, ranking defaults and what SDI does to pull requests on its own. Each section saves on its own; environment values are shown, not edited."
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
          <Panel
            title="Automation"
            description="What SDI does to pull requests without being asked. With everything off, nothing is reviewed, drafted or merged on its own."
            padding="none"
            skeleton={<AutomationSkeleton />}
          >
            <AutomationPanel />
          </Panel>
        </div>
        <div className="flex flex-col gap-4 lg:col-span-4">
          <ReasonerUsageStat />
          <Panel title="Environment" description="Read from the running services." padding="none" skeleton={<EnvironmentSkeleton />}>
            <EnvironmentPanel />
          </Panel>
        </div>
      </div>
    </Page>
  )
}
