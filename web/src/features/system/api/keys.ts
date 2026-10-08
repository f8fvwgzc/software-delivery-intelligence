/** Query key family for gateway health and the three store counters. */
export const systemKeys = {
  all: ['system'] as const,
  status: () => [...systemKeys.all, 'status'] as const,
  knowledgeStats: () => [...systemKeys.all, 'knowledge-stats'] as const,
  intelligenceStats: () => [...systemKeys.all, 'intelligence-stats'] as const,
  deliveryStats: () => [...systemKeys.all, 'delivery-stats'] as const,
}
