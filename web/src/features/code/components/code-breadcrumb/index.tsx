import { ExternalLinkIcon, GitCommitHorizontalIcon } from 'lucide-react'
import { Fragment } from 'react'
import { Link } from 'react-router'

import { routes } from '@/app/router/routes'
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from '@/components/ui/breadcrumb'
import { Button } from '@/components/ui/button'
import { Skeleton } from '@/components/ui/skeleton'
import type { Project } from '@/features/projects/api/queries'

import { shortSha } from '../../lib/format'
import { splitPath } from '../../lib/path'

interface CodeBreadcrumbProps {
  project: Pick<Project, 'id' | 'name' | 'repoUrl'>
  commitSha: string
  /** `''` at the repository root. */
  path: string
  isDir: boolean
}

function githubUrl(repoUrl: string, commitSha: string, path: string, isDir: boolean) {
  const base = repoUrl.replace(/\/+$/, '')
  const kind = isDir ? 'tree' : 'blob'
  const encoded = splitPath(path).map(encodeURIComponent).join('/')
  return encoded ? `${base}/${kind}/${commitSha}/${encoded}` : `${base}/tree/${commitSha}`
}

/** repo › dir › file, every segment a link, with the commit and the GitHub escape hatch. */
export function CodeBreadcrumb({ project, commitSha, path, isDir }: CodeBreadcrumbProps) {
  const segments = splitPath(path)
  const base = project.repoUrl.replace(/\/+$/, '')

  return (
    <div className="flex min-h-8 flex-wrap items-center gap-x-4 gap-y-2">
      <Breadcrumb className="min-w-0 flex-1">
        <BreadcrumbList className="flex-nowrap gap-1 font-mono text-sm">
          <BreadcrumbItem>
            {segments.length === 0 ? (
              <BreadcrumbPage className="font-sans font-medium">{project.name}</BreadcrumbPage>
            ) : (
              <BreadcrumbLink asChild className="font-sans font-medium">
                <Link to={routes.projectCode(project.id)}>{project.name}</Link>
              </BreadcrumbLink>
            )}
          </BreadcrumbItem>
          {segments.map((segment, index) => {
            const isLast = index === segments.length - 1
            const segmentPath = segments.slice(0, index + 1).join('/')
            return (
              <Fragment key={segmentPath}>
                <BreadcrumbSeparator />
                <BreadcrumbItem className="min-w-0">
                  {isLast ? (
                    <BreadcrumbPage className="truncate">{segment}</BreadcrumbPage>
                  ) : (
                    <BreadcrumbLink asChild className="truncate">
                      <Link to={routes.projectCode(project.id, segmentPath)}>{segment}</Link>
                    </BreadcrumbLink>
                  )}
                </BreadcrumbItem>
              </Fragment>
            )
          })}
        </BreadcrumbList>
      </Breadcrumb>

      <div className="flex shrink-0 items-center gap-2">
        <a
          href={`${base}/commit/${commitSha}`}
          target="_blank"
          rel="noreferrer"
          title={commitSha}
          className="inline-flex h-7 items-center gap-1.5 rounded-md px-1.5 font-mono text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          <GitCommitHorizontalIcon className="size-3.5" aria-hidden="true" />
          {shortSha(commitSha)}
        </a>
        <Button variant="outline" size="sm" asChild>
          <a href={githubUrl(project.repoUrl, commitSha, path, isDir)} target="_blank" rel="noreferrer">
            Open on GitHub
            <ExternalLinkIcon data-icon="inline-end" />
          </a>
        </Button>
      </div>
    </div>
  )
}

export function CodeBreadcrumbSkeleton() {
  return (
    <div className="flex min-h-8 items-center gap-4">
      <div className="flex flex-1 items-center gap-2">
        <Skeleton className="h-4 w-24" />
        <Skeleton className="h-4 w-3" />
        <Skeleton className="h-4 w-16" />
      </div>
      <Skeleton className="h-4 w-16" />
      <Skeleton className="h-7 w-32 rounded-lg" />
    </div>
  )
}
