import type { RegistryAuth, RegistryKind } from '../api/types'

export interface RegistryKindMeta {
  label: string
  description: string
  /** Fixed host, or '' when it depends on the account (ACR, ECR, GAR, generic). */
  host: string
  hostPlaceholder: string
  namespacePlaceholder: string
  /** The ways this registry can be signed in to, the first one the default. */
  auth: readonly RegistryAuth[]
}

/** The registry kinds of CONTRACT.md §22. */
export const REGISTRY_KINDS: Record<RegistryKind, RegistryKindMeta> = {
  ghcr: {
    label: 'GitHub Container Registry',
    description: 'ghcr.io, signed in with the workflow’s own GITHUB_TOKEN',
    host: 'ghcr.io',
    hostPlaceholder: 'ghcr.io',
    namespacePlaceholder: 'acme',
    auth: ['github_token', 'secret'],
  },
  dockerhub: {
    label: 'Docker Hub',
    description: 'docker.io, signed in with a username and an access token',
    host: 'docker.io',
    hostPlaceholder: 'docker.io',
    namespacePlaceholder: 'acme',
    auth: ['secret'],
  },
  acr: {
    label: 'Azure Container Registry',
    description: 'Signed in from CI through OIDC',
    host: '',
    hostPlaceholder: 'acme.azurecr.io',
    namespacePlaceholder: 'services',
    auth: ['oidc', 'secret'],
  },
  ecr: {
    label: 'Amazon ECR',
    description: 'Signed in from CI through OIDC',
    host: '',
    hostPlaceholder: '123456789012.dkr.ecr.eu-west-1.amazonaws.com',
    namespacePlaceholder: 'services',
    auth: ['oidc', 'secret'],
  },
  gar: {
    label: 'Google Artifact Registry',
    description: 'Signed in from CI through OIDC',
    host: '',
    hostPlaceholder: 'europe-west1-docker.pkg.dev',
    namespacePlaceholder: 'my-project/containers',
    auth: ['oidc', 'secret'],
  },
  generic: {
    label: 'Other registry',
    description: 'Any Registry v2 endpoint, signed in with a username and a password',
    host: '',
    hostPlaceholder: 'registry.example.com',
    namespacePlaceholder: 'acme',
    auth: ['secret'],
  },
}

export const REGISTRY_KIND_ORDER = Object.keys(REGISTRY_KINDS) as RegistryKind[]

export function registryKind(kind: string): RegistryKindMeta {
  return REGISTRY_KINDS[kind as RegistryKind] ?? REGISTRY_KINDS.generic
}

export const REGISTRY_AUTH: Record<RegistryAuth, { label: string; description: string }> = {
  github_token: { label: 'GitHub token', description: 'The workflow’s GITHUB_TOKEN with packages: write. Nothing to configure' },
  oidc: { label: 'OIDC from CI', description: 'The cloud trusts the workflow’s identity. No stored credentials' },
  secret: { label: 'Workspace secrets', description: 'A username and a password, referenced by secret name' },
}

export function registryAuthLabel(auth: string) {
  return REGISTRY_AUTH[auth as RegistryAuth]?.label ?? auth
}
