import { routes } from '@/app/router/routes'

/** Where a diagram node or an evidence path opens: the code browser at `/projects/:id/code/*path` (CONTRACT.md §15). */
export function codeBrowserPath(projectId: string, path: string) {
  return routes.projectCode(projectId, path)
}
