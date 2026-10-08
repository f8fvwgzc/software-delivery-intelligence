import type { components } from '@/lib/api/schema'

export interface ApprovalListFilters {
  status?: components['schemas']['ApprovalStatus']
  executionId?: string
}

export const approvalKeys = {
  all: ['approvals'] as const,
  lists: () => [...approvalKeys.all, 'list'] as const,
  list: (filters: ApprovalListFilters) => [...approvalKeys.lists(), filters] as const,
}
