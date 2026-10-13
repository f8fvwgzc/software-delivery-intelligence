import type { components } from '@/lib/api/schema'

/*
 * Security DTOs come from the gateway's OpenAPI contract (CONTRACT.md
 * §23.4). Only the string unions the schema leaves open (severity, source,
 * the state of Dependabot's two switches, scope) are narrowed here.
 */

type Schemas = components['schemas']

export type SecurityOverview = Schemas['SecurityOverview']
export type Vulnerability = Schemas['Vulnerability']
export type SeverityCount = Schemas['SeverityCount']
export type ConfigureDependabotRequest = Schemas['ConfigureDependabotRequest']
export type ConfigureDependabotResult = Schemas['ConfigureDependabotResult']

/** `Vulnerability.severity` and `SeverityCount.severity`. */
export type VulnerabilitySeverity = 'critical' | 'high' | 'medium' | 'low' | 'unknown'
/** `SecurityOverview.source`: where the list came from. */
export type SecuritySource = 'dependabot' | 'osv' | 'none'
/** `SecurityOverview.alerts`. */
export type AlertsState = 'enabled' | 'disabled' | 'unknown'
/** `SecurityOverview.securityUpdates`. */
export type SecurityUpdatesState = 'enabled' | 'paused' | 'disabled' | 'unknown'
/** `Vulnerability.scope`. */
export type VulnerabilityScope = 'runtime' | 'development'
