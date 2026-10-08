import type { components } from '@/lib/api/schema'

/** Aliases over the generated contract so components never reach into `components['schemas']`. */
export type Pipeline = components['schemas']['Pipeline']
export type PipelineJob = components['schemas']['PipelineJob']
export type PipelineRun = components['schemas']['PipelineRun']
export type PipelineRunsResponse = components['schemas']['PipelineRunsResponse']
export type PipelineStats = components['schemas']['PipelineStats']
export type RunStatus = components['schemas']['RunStatus']
export type RunFailure = components['schemas']['RunFailure']
export type Deployment = components['schemas']['Deployment']
export type DeploymentsResponse = components['schemas']['DeploymentsResponse']
export type DeploymentStats = components['schemas']['DeploymentStats']
export type DeploymentStatus = components['schemas']['DeploymentStatus']
