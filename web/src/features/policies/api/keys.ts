export const policyKeys = {
  all: ['policies'] as const,
  list: () => [...policyKeys.all, 'list'] as const,
}

export const identityKeys = {
  all: ['identities'] as const,
  list: () => [...identityKeys.all, 'list'] as const,
}
