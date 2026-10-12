import { usePrefetchQuery, useSuspenseQuery } from '@tanstack/react-query'

import { Page, PageHeader } from '@/components/shared/page'
import { Panel } from '@/components/system/panel'
import { githubQueries } from '@/features/github/api/queries'

import { systemQueries } from '../api/queries'
import { DeliverySummary } from '../components/delivery-summary'
import { GatewaySummary } from '../components/gateway-summary'
import { GithubSummary } from '../components/github-summary'
import { IntelligenceSummary } from '../components/intelligence-summary'
import { KeyValueSkeleton } from '../components/key-value-skeleton'
import { KnowledgeSummary } from '../components/knowledge-summary'
import { RefreshButton } from '../components/refresh-button'
import { ServiceTable, ServiceTableSkeleton } from '../components/service-table'

/*
 * Each panel owns one query and one boundary, so a store whose service is
 * down renders its own error tile while the rest of the page stays useful.
 */

function ServicesPanel() {
  const { data } = useSuspenseQuery(systemQueries.status())
  return <ServiceTable services={data.services} />
}

function GatewayPanel() {
  const { data } = useSuspenseQuery(systemQueries.status())
  return <GatewaySummary status={data} />
}

function GithubPanel() {
  const { data } = useSuspenseQuery(githubQueries.account())
  return <GithubSummary account={data} />
}

function KnowledgePanel() {
  const { data } = useSuspenseQuery(systemQueries.knowledgeStats())
  return <KnowledgeSummary stats={data} />
}

function IntelligencePanel() {
  const { data } = useSuspenseQuery(systemQueries.intelligenceStats())
  return <IntelligenceSummary stats={data} />
}

function DeliveryPanel() {
  const { data } = useSuspenseQuery(systemQueries.deliveryStats())
  return <DeliverySummary stats={data} />
}

export default function SystemPage() {
  // Start every request in the same tick; the panels below only await them.
  usePrefetchQuery(systemQueries.status())
  usePrefetchQuery(githubQueries.account())
  usePrefetchQuery(systemQueries.knowledgeStats())
  usePrefetchQuery(systemQueries.intelligenceStats())
  usePrefetchQuery(systemQueries.deliveryStats())

  return (
    <Page>
      <PageHeader
        title="System"
        description="Health of the gateway and its backing services, and the counters of each store."
        actions={<RefreshButton />}
      />

      <Panel
        title="Services"
        description="gRPC health checks run by the gateway, with the round trip of each."
        padding="none"
        skeleton={<ServiceTableSkeleton />}
      >
        <ServicesPanel />
      </Panel>

      <div className="grid gap-4 lg:grid-cols-2 lg:items-start">
        <div className="flex flex-col gap-4">
          <Panel title="Gateway" skeleton={<KeyValueSkeleton rows={4} />}>
            <GatewayPanel />
          </Panel>
          <Panel title="GitHub account" description="The account behind the configured token." skeleton={<KeyValueSkeleton rows={5} />}>
            <GithubPanel />
          </Panel>
          <Panel title="Delivery" description="Execution counters across every project." skeleton={<KeyValueSkeleton rows={10} />}>
            <DeliveryPanel />
          </Panel>
        </div>

        <div className="flex flex-col gap-4">
          <Panel title="Knowledge store" description="Facts, graph and semantic index." skeleton={<KeyValueSkeleton rows={7} />}>
            <KnowledgePanel />
          </Panel>
          <Panel title="Intelligence store" description="Proposals, decisions and documents." skeleton={<KeyValueSkeleton rows={6} />}>
            <IntelligencePanel />
          </Panel>
        </div>
      </div>
    </Page>
  )
}
