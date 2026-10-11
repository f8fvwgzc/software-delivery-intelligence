/** Session state: who is signed in and which workspaces they can open. */
export const authKeys = {
  all: ['auth'] as const,
  me: () => [...authKeys.all, 'me'] as const,
  installUrl: () => [...authKeys.all, 'install-url'] as const,
}
