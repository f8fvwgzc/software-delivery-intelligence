import { useQuery, useSuspenseQuery } from '@tanstack/react-query'

import { QueryBoundary } from '@/components/shared/feedback'
import { Page, PageHeader, Section } from '@/components/shared/page'
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
      <div className="grid gap-6 lg:grid-cols-12">
        <div className="flex flex-col gap-6 lg:col-span-8">
          <Section title="Scanner" description="How much of a repository one scan reads, and how fast.">
            <QueryBoundary fallback={<FormSkeleton fields={4} toggle />}>
              <ScannerPanel />
            </QueryBoundary>
          </Section>
          <Section title="Ranking" description="How proposal options are scored and who reasons about them.">
            <QueryBoundary fallback={<FormSkeleton fields={7} columns={4} toggle />}>
              <RankingPanel />
            </QueryBoundary>
          </Section>
        </div>
        <Section title="Environment" description="Read from the running services." className="lg:col-span-4">
          <QueryBoundary fallback={<EnvironmentSkeleton />}>
            <EnvironmentPanel />
          </QueryBoundary>
        </Section>
      </div>
    </Page>
  )
}
