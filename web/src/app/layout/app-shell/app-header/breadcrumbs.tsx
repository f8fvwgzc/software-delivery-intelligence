import { Fragment, Suspense } from 'react'
import { Link, useMatches } from 'react-router'

import type { RouteHandle } from '@/app/router'
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from '@/components/ui/breadcrumb'
import { Skeleton } from '@/components/ui/skeleton'

function hasCrumb(handle: unknown): handle is RouteHandle {
  return typeof handle === 'object' && handle !== null && 'crumb' in handle
}

/** Built from the `handle.crumb` of every matched route. */
export function Breadcrumbs() {
  const matches = useMatches().filter((match) => hasCrumb(match.handle))
  const last = matches.length - 1

  return (
    <Breadcrumb>
      <BreadcrumbList className="text-xs sm:gap-1.5">
        {matches.map((match, index) => {
          const { crumb } = match.handle as RouteHandle
          const label =
            typeof crumb === 'string' ? (
              crumb
            ) : (
              <Suspense fallback={<Skeleton className="h-3.5 w-20" />}>
                {(() => {
                  const Crumb = crumb
                  return <Crumb />
                })()}
              </Suspense>
            )

          return (
            <Fragment key={match.id}>
              {index > 0 && <BreadcrumbSeparator />}
              <BreadcrumbItem>
                {index === last ? (
                  <BreadcrumbPage className="font-medium">{label}</BreadcrumbPage>
                ) : (
                  <BreadcrumbLink asChild>
                    <Link to={match.pathname}>{label}</Link>
                  </BreadcrumbLink>
                )}
              </BreadcrumbItem>
            </Fragment>
          )
        })}
      </BreadcrumbList>
    </Breadcrumb>
  )
}
