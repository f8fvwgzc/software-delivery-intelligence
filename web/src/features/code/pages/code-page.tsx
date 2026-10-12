import { useSuspenseQuery } from '@tanstack/react-query'
import { FileQuestionIcon, RadarIcon, TriangleAlertIcon } from 'lucide-react'
import { useParams } from 'react-router'

import { EmptyState, QueryBoundary } from '@/components/shared/feedback'
import { Page } from '@/components/shared/page'
import { projectQueries, type Project } from '@/features/projects/api/queries'

import { codeQueries, type RepoTree } from '../api/queries'
import { CodeBreadcrumb, CodeBreadcrumbSkeleton } from '../components/code-breadcrumb'
import { DirectoryView, DirectoryViewSkeleton } from '../components/directory-view'
import { FileTree, FileTreeSkeleton } from '../components/file-tree'
import { FileBoundary, FileView, FileViewSkeleton } from '../components/file-view'
import { normalizePath } from '../lib/path'
import { buildTree, type TreeModel } from '../lib/tree'

const GRID_CLASS = 'grid gap-4 lg:grid-cols-[18rem_minmax(0,1fr)] lg:items-start'
/**
 * The tree pane sticks 0.5rem under the app header (`--header-h`) plus the project
 * band (`--project-band`, measured by the project layout) and scrolls on its own.
 */
const TREE_PANE_CLASS =
  'flex max-h-72 flex-col gap-2 rounded-lg p-2 ring-1 ring-border lg:sticky lg:top-[calc(var(--header-h)+0.5rem+var(--project-band,0px))] lg:max-h-[calc(100dvh-var(--header-h)-1.5rem-var(--project-band,0px))]'
/** No overflow clipping here: the file header is sticky against the window, not the pane. */
const CONTENT_PANE_CLASS = 'min-w-0 rounded-lg ring-1 ring-border'

/** Stable `select`: the nested tree is built once per tree response. */
function toTreeModel(tree: RepoTree): TreeModel {
  return buildTree(tree)
}

interface ContentProps {
  projectId: string
  model: TreeModel
  path: string
}

/** Directory listing, file, or an explanation of why neither exists. */
function Content({ projectId, model, path }: ContentProps) {
  if (path === '') {
    return <DirectoryView projectId={projectId} path="" entries={model.roots} />
  }
  const node = model.byPath.get(path)
  if (node?.isDir) {
    return <DirectoryView projectId={projectId} path={path} entries={node.children} />
  }
  if (node || model.truncated) {
    // A truncated tree may be missing the entry; the file endpoint still knows it.
    return (
      <FileBoundary fallback={<FileViewSkeleton />}>
        <FileView key={path} projectId={projectId} path={path} />
      </FileBoundary>
    )
  }
  return (
    <EmptyState
      icon={FileQuestionIcon}
      title="Nothing at this path"
      description="The scanned commit has no file or directory here. Pick one from the tree."
      className="m-4"
    />
  )
}

function CodeBrowser({ project, path }: { project: Project; path: string }) {
  const { data: model } = useSuspenseQuery({ ...codeQueries.tree(project.id), select: toTreeModel })
  const node = model.byPath.get(path)
  const isDir = path === '' || node?.isDir === true

  return (
    <>
      <CodeBreadcrumb project={project} commitSha={model.commitSha} path={path} isDir={isDir} />
      {model.truncated ? (
        <p className="flex items-center gap-1.5 text-xs text-warning">
          <TriangleAlertIcon className="size-3.5 shrink-0" aria-hidden="true" />
          GitHub truncated this tree; very deep paths may be missing from the listing but still open by URL.
        </p>
      ) : null}
      <div className={GRID_CLASS}>
        <aside className={TREE_PANE_CLASS} aria-label="File tree">
          <FileTree model={model} projectId={project.id} currentPath={path} className="flex min-h-0 flex-1 flex-col gap-2" />
        </aside>
        <section className={CONTENT_PANE_CLASS}>
          <Content projectId={project.id} model={model} path={path} />
        </section>
      </div>
    </>
  )
}

function CodeBrowserSkeleton() {
  return (
    <>
      <CodeBreadcrumbSkeleton />
      <div className={GRID_CLASS}>
        <aside className={TREE_PANE_CLASS}>
          <FileTreeSkeleton />
        </aside>
        <section className={CONTENT_PANE_CLASS}>
          <DirectoryViewSkeleton />
        </section>
      </div>
    </>
  )
}

/** `/projects/:projectId/code/*`: the repository at the scanned commit. */
export default function CodePage() {
  const { projectId = '', '*': splat } = useParams()
  const path = normalizePath(splat)
  const { data: project } = useSuspenseQuery(projectQueries.detail(projectId))

  if (project.status !== 'ready') {
    return (
      <Page>
        <EmptyState
          icon={RadarIcon}
          title="Code appears after the first scan"
          description="The tree and file contents are read at the commit a successful scan analysed."
        />
      </Page>
    )
  }

  return (
    <Page>
      <QueryBoundary fallback={<CodeBrowserSkeleton />}>
        <CodeBrowser project={project} path={path} />
      </QueryBoundary>
    </Page>
  )
}
