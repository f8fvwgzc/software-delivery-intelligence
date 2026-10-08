import { PackageSearchIcon } from 'lucide-react'

import { EmptyState } from '@/components/shared/feedback'
import { Button } from '@/components/ui/button'

import type { DependenciesResponse } from '../../api/queries'
import { DependabotStatus } from '../dependabot-status'
import { EcosystemSummary } from '../ecosystem-summary'
import { PAGE_SIZE } from './columns'
import { DependenciesTable } from './dependencies-table'
import { TablePagination } from './pagination'
import { DependencyToolbar } from './toolbar'
import { useDependencyFilters } from './use-dependency-filters'

/**
 * Summary on top, filterable table below. The ecosystem filter is shared
 * between the bars and the select, so either one scopes the table.
 */
export function DependencyExplorer({ data }: { data: DependenciesResponse }) {
  const filters = useDependencyFilters(data.dependencies)
  const total = data.dependencies.length

  return (
    <div className="flex flex-col gap-6">
      <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <EcosystemSummary
          counts={data.byEcosystem}
          total={total}
          selected={filters.ecosystem}
          onSelect={filters.setEcosystem}
        />
        <DependabotStatus status={data.dependabot} />
      </div>

      <div className="flex flex-col gap-3">
        <DependencyToolbar
          counts={data.byEcosystem}
          ecosystem={filters.ecosystem}
          onEcosystemChange={filters.setEcosystem}
          search={filters.search}
          onSearchChange={filters.setSearch}
          shown={filters.filtered.length}
          total={total}
          isFiltered={filters.isFiltered}
          onClear={filters.clear}
        />

        {filters.rows.length > 0 ? (
          <DependenciesTable rows={filters.rows} stale={filters.isStale} />
        ) : (
          <EmptyState
            icon={PackageSearchIcon}
            title="No dependencies match"
            description="Try another ecosystem or a shorter search."
            action={
              <Button variant="outline" size="sm" onClick={filters.clear}>
                Reset filters
              </Button>
            }
            className="py-8"
          />
        )}

        {filters.filtered.length > PAGE_SIZE ? (
          <TablePagination page={filters.page} pageCount={filters.pageCount} onPageChange={filters.setPage} />
        ) : null}
      </div>
    </div>
  )
}

export { DependencyExplorerSkeleton } from './explorer-skeleton'
