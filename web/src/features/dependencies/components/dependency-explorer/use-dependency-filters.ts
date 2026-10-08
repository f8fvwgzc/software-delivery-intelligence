import { useDeferredValue, useMemo, useState } from 'react'

import type { Dependency } from '../../api/queries'
import { ALL_ECOSYSTEMS } from '../../lib/labels'
import { PAGE_SIZE } from './columns'

function matches(dependency: Dependency, ecosystem: string, needle: string) {
  if (ecosystem !== ALL_ECOSYSTEMS && dependency.ecosystem !== ecosystem) return false
  if (!needle) return true
  return (
    dependency.name.toLowerCase().includes(needle) || dependency.manifestPath.toLowerCase().includes(needle)
  )
}

function byEcosystemThenName(a: Dependency, b: Dependency) {
  return a.ecosystem.localeCompare(b.ecosystem) || a.name.localeCompare(b.name)
}

/**
 * Filter, sort and page a dependency list on the client. Everything shown
 * is derived during render; the only state is what the user typed or chose.
 */
export function useDependencyFilters(dependencies: Dependency[]) {
  const [ecosystem, setEcosystemState] = useState(ALL_ECOSYSTEMS)
  const [search, setSearchState] = useState('')
  const [page, setPage] = useState(1)

  // Typing stays responsive even when the list is long.
  const deferredSearch = useDeferredValue(search)

  const filtered = useMemo(() => {
    const needle = deferredSearch.trim().toLowerCase()
    return dependencies.filter((dependency) => matches(dependency, ecosystem, needle)).sort(byEcosystemThenName)
  }, [dependencies, ecosystem, deferredSearch])

  const pageCount = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE))
  const currentPage = Math.min(page, pageCount)
  const rows = filtered.slice((currentPage - 1) * PAGE_SIZE, currentPage * PAGE_SIZE)

  const setEcosystem = (value: string) => {
    setEcosystemState(value)
    setPage(1)
  }

  const setSearch = (value: string) => {
    setSearchState(value)
    setPage(1)
  }

  const clear = () => {
    setEcosystemState(ALL_ECOSYSTEMS)
    setSearchState('')
    setPage(1)
  }

  return {
    ecosystem,
    search,
    isFiltered: ecosystem !== ALL_ECOSYSTEMS || search.trim() !== '',
    isStale: search !== deferredSearch,
    filtered,
    rows,
    page: currentPage,
    pageCount,
    setEcosystem,
    setSearch,
    setPage,
    clear,
  }
}
