/** Query key family for the GitHub account and the repositories its token can see. */
export const githubKeys = {
  all: ['github'] as const,
  account: () => [...githubKeys.all, 'account'] as const,
  repos: () => [...githubKeys.all, 'repos'] as const,
  repoList: (query: string) => [...githubKeys.repos(), { query }] as const,
}
