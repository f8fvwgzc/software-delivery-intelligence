import { GITOPS_DIFF_ROOT, GITOPS_MANIFESTS_ROOT } from '@/lib/live/event-families'

/*
 * Four roots, so the live stream and the mutations can target them:
 *
 * - `gitops`: what the service answers from its own store (the list, an
 *   application with its resources, its history). Every `application`
 *   event invalidates it.
 * - `gitops-diff` and `gitops-manifests`: answers the service has to
 *   compute (render the source, read the cluster). They are fetched only
 *   when someone opens them, and the live stream reaches them through the
 *   gated lane (lib/live/event-families.ts), by application, never on an
 *   operation's progress.
 * - `clusters`: workspace configuration, changed only by an admin's own
 *   mutations.
 */
export const gitopsKeys = {
  all: ['gitops'] as const,
  applications: () => [...gitopsKeys.all, 'applications'] as const,
  lists: () => [...gitopsKeys.applications(), 'list'] as const,
  list: (projectId: string) => [...gitopsKeys.lists(), projectId] as const,
  details: () => [...gitopsKeys.applications(), 'detail'] as const,
  detail: (applicationId: string) => [...gitopsKeys.details(), applicationId] as const,
  history: (applicationId: string) => [...gitopsKeys.all, 'history', applicationId] as const,
}

export const diffKeys = {
  all: [GITOPS_DIFF_ROOT] as const,
  application: (applicationId: string) => [...diffKeys.all, applicationId] as const,
}

export const manifestKeys = {
  all: [GITOPS_MANIFESTS_ROOT] as const,
  application: (applicationId: string) => [...manifestKeys.all, applicationId] as const,
}

export const clusterKeys = {
  all: ['clusters'] as const,
  list: () => [...clusterKeys.all, 'list'] as const,
}
