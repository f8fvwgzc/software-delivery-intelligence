import type { components } from '@/lib/api/schema'

/*
 * Continuous delivery DTOs come from the gateway's OpenAPI contract
 * (CONTRACT.md §22). Only the string unions the schema leaves open (kinds,
 * auth, health) are narrowed here. Optional strings the services leave
 * empty arrive as `null`.
 */

type Schemas = components['schemas']

export type Artifact = Schemas['Artifact']
export type Registry = Schemas['Registry']
export type UpsertRegistryRequest = Schemas['UpsertRegistryRequest']
export type Environment = Schemas['Environment']
export type UpsertEnvironmentRequest = Schemas['UpsertEnvironmentRequest']
export type Release = Schemas['Release']
export type ReleaseInspection = Schemas['ReleaseInspection']
export type InspectionReport = Schemas['InspectionReport']
export type InspectionFinding = Schemas['InspectionFinding']
export type InspectedBinary = Schemas['InspectedBinary']
export type InspectedComponent = Schemas['InspectedComponent']
export type CreateReleaseRequest = Schemas['CreateReleaseRequest']
export type DeploymentRun = Schemas['DeploymentRun']
export type DeploymentRunStatus = Schemas['DeploymentRunStatus']
export type PipelineStage = Schemas['PipelineStage']
export type DeliveryPipeline = Schemas['DeliveryPipeline']
export type PromoteRequest = Schemas['PromoteRequest']
export type RollbackRequest = Schemas['RollbackRequest']
export type DeliveryArtifacts = Schemas['DeliveryArtifacts']
export type SecretInfo = Schemas['SecretInfo']
export type SetSecretRequest = Schemas['SetSecretRequest']

/** `Registry.kind`. */
export type RegistryKind = 'ghcr' | 'dockerhub' | 'acr' | 'ecr' | 'gar' | 'generic'
/** `Registry.auth`. */
export type RegistryAuth = 'github_token' | 'oidc' | 'secret'
/** `Environment.kind`: the deployer. */
export type EnvironmentKind =
  | 'github_actions'
  | 'gitops_flux'
  | 'gitops_argocd'
  | 'jenkins'
  | 'azure_devops'
  | 'webhook'
  | 'docker_local'
/** `DeploymentRun.kind`. */
export type DeploymentKind = 'promote' | 'rollback'
/** `PipelineStage.health`. */
export type StageHealth = 'healthy' | 'unhealthy' | 'unknown'
