import type { EnvironmentKind } from '../api/types'

export type ConfigFieldType = 'text' | 'number' | 'secret' | 'choice' | 'json'

export interface ConfigChoice {
  value: string
  label: string
  description: string
}

/** One key of `Environment.config`, as the editor asks for it. */
export interface ConfigField {
  /** The config key, exactly as CONTRACT.md §22 names it. */
  key: string
  label: string
  type: ConfigFieldType
  /** The value the deployer falls back to, shown as the placeholder. */
  placeholder?: string
  description?: string
  required?: boolean
  choices?: readonly ConfigChoice[]
  /** Hidden unless this holds for the config as typed so far. */
  when?: (config: Record<string, string>) => boolean
}

export interface DeployerMeta {
  /** The tag on a stage of the board. */
  tag: string
  label: string
  /** What SDI does with it, in one sentence. */
  description: string
  fields: readonly ConfigField[]
}

const GITOPS_STRATEGIES: readonly ConfigChoice[] = [
  { value: 'kustomize_image', label: 'Kustomize image', description: 'Sets newTag under images in a kustomization.yaml' },
  { value: 'helm_values', label: 'Helm values', description: 'Sets the tag at a path in a values file' },
  { value: 'yaml_path', label: 'YAML path', description: 'Sets the value at a path in any YAML file' },
]

const GITOPS_MODES: readonly ConfigChoice[] = [
  { value: 'pull_request', label: 'Pull request', description: 'Opens a pull request and waits for the merge' },
  { value: 'commit', label: 'Commit', description: 'Commits straight to the branch' },
]

const needsYamlPath = (config: Record<string, string>) => config.strategy === 'helm_values' || config.strategy === 'yaml_path'

/** Both GitOps controllers take the same settings: SDI edits the repository, the controller reconciles. */
const GITOPS_FIELDS: readonly ConfigField[] = [
  { key: 'repo', label: 'GitOps repository', type: 'text', placeholder: 'owner/name', description: 'Defaults to this project’s repository.' },
  { key: 'branch', label: 'Branch', type: 'text', placeholder: 'main', description: 'Defaults to the repository’s default branch.' },
  { key: 'path', label: 'File', type: 'text', placeholder: 'apps/api/kustomization.yaml', description: 'The file that pins the image tag.', required: true },
  { key: 'strategy', label: 'Strategy', type: 'choice', choices: GITOPS_STRATEGIES, required: true },
  { key: 'yaml_path', label: 'YAML path', type: 'text', placeholder: 'image.tag', description: 'Where the tag lives inside the file.', required: true, when: needsYamlPath },
  { key: 'mode', label: 'Mode', type: 'choice', choices: GITOPS_MODES, placeholder: 'Pull request' },
]

/** The deployers of CONTRACT.md §22, with exactly the config keys the table lists. */
export const DEPLOYERS: Record<EnvironmentKind, DeployerMeta> = {
  github_actions: {
    tag: 'github actions',
    label: 'GitHub Actions',
    description: 'Dispatches the deploy workflow and follows that run to completion',
    fields: [
      { key: 'workflow', label: 'Workflow', type: 'text', placeholder: 'deploy.yml', description: 'Needs workflow_dispatch with the inputs environment, image, digest and version.' },
      { key: 'ref', label: 'Ref', type: 'text', placeholder: 'main', description: 'Defaults to the default branch.' },
    ],
  },
  gitops_flux: {
    tag: 'flux',
    label: 'Flux (GitOps)',
    description: 'Edits the image tag in the GitOps repository; Flux reconciles',
    fields: GITOPS_FIELDS,
  },
  gitops_argocd: {
    tag: 'argo cd',
    label: 'Argo CD (GitOps)',
    description: 'Edits the image tag in the GitOps repository; Argo CD reconciles',
    fields: GITOPS_FIELDS,
  },
  jenkins: {
    tag: 'jenkins',
    label: 'Jenkins',
    description: 'Triggers a parameterised build and polls it',
    fields: [
      { key: 'url', label: 'Jenkins URL', type: 'text', placeholder: 'https://jenkins.example.com', required: true },
      { key: 'job', label: 'Job', type: 'text', placeholder: 'deploy-api', required: true },
      { key: 'user_secret', label: 'User', type: 'secret', description: 'The workspace secret holding the Jenkins user.' },
      { key: 'token_secret', label: 'API token', type: 'secret', description: 'The workspace secret holding that user’s API token.' },
    ],
  },
  azure_devops: {
    tag: 'azure devops',
    label: 'Azure DevOps',
    description: 'Runs an Azure Pipelines pipeline and polls the run',
    fields: [
      { key: 'organization', label: 'Organization', type: 'text', placeholder: 'acme', required: true },
      { key: 'project', label: 'Project', type: 'text', placeholder: 'platform', required: true },
      { key: 'pipeline_id', label: 'Pipeline id', type: 'number', placeholder: '42', required: true },
      { key: 'pat_secret', label: 'Personal access token', type: 'secret', description: 'The workspace secret holding a token that may run pipelines.', required: true },
    ],
  },
  webhook: {
    tag: 'webhook',
    label: 'Webhook',
    description: 'Sends a signed POST with the release; a 2xx answer is success',
    fields: [
      { key: 'url', label: 'Webhook URL', type: 'text', placeholder: 'https://deploy.example.com/hooks/sdi', required: true },
      { key: 'secret', label: 'Signing secret', type: 'secret', description: 'Signs the body as HMAC-SHA256 in X-SDI-Signature-256.', required: true },
      { key: 'timeout_seconds', label: 'Timeout (seconds)', type: 'number', description: 'How long to wait for the answer.' },
    ],
  },
  docker_local: {
    tag: 'local',
    label: 'Local Docker',
    description: 'Runs the image on this host’s Docker. Development only',
    fields: [
      { key: 'port', label: 'Port', type: 'number', placeholder: '8080', description: 'Host port the container is published on.' },
      { key: 'env', label: 'Environment variables', type: 'json', placeholder: '{"LOG_LEVEL": "debug"}', description: 'A JSON object of name and value.' },
    ],
  },
}

/** In the order the editor offers them. */
export const DEPLOYER_KINDS = Object.keys(DEPLOYERS) as EnvironmentKind[]

/** Unknown kinds (a newer gateway, a typo in `.sdi.yml`) still render, with no fields to edit. */
export function deployerOf(kind: string): DeployerMeta {
  return DEPLOYERS[kind as EnvironmentKind] ?? { tag: kind.replaceAll('_', ' '), label: kind, description: '', fields: [] }
}

/** The fields that apply to the config as typed so far. */
export function visibleFields(kind: string, config: Record<string, string>): ConfigField[] {
  return deployerOf(kind).fields.filter((field) => !field.when || field.when(config))
}
