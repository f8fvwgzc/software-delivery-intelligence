import type { Role } from '@/features/auth/api/types'
import { hasRole } from '@/features/auth/lib/roles'

import type { ConfigureDependabotRequest, SecurityOverview } from '../api/types'

/*
 * The "Turn on Dependabot" dialog as pure functions (CONTRACT.md §23.4):
 * which of its three options are already on, which start ticked, and the
 * request a selection becomes.
 */

export type DependabotOption = 'alerts' | 'securityUpdates' | 'configFile'

export type DependabotSelection = Record<DependabotOption, boolean>

type DependabotState = Pick<SecurityOverview, 'alerts' | 'securityUpdates' | 'configPresent' | 'configPath'>

export interface DependabotOptionState {
  key: DependabotOption
  label: string
  description: string
  /** Already on: shown ticked and locked, and never sent. */
  on: boolean
  /** Said next to an option that is on, or whose state GitHub did not tell. */
  note: string | null
  /** Starts ticked: known to be off. An unknown state is left for the person to decide. */
  preselected: boolean
}

/** The three options in the order they take effect, each with what GitHub says about it now. */
export function dependabotOptions(state: DependabotState): DependabotOptionState[] {
  const alertsOn = state.alerts === 'enabled'
  const updatesOn = state.securityUpdates === 'enabled'
  const unknown = 'GitHub did not say whether this is on.'

  return [
    {
      key: 'alerts',
      label: 'Dependabot alerts',
      description: 'GitHub tells you when a dependency of this repository has a known vulnerability.',
      on: alertsOn,
      note: alertsOn ? 'Already on' : state.alerts === 'unknown' ? unknown : null,
      preselected: state.alerts === 'disabled',
    },
    {
      key: 'securityUpdates',
      label: 'Security updates',
      description: 'Dependabot opens a pull request that fixes each alert. They need alerts, so alerts are turned on with them.',
      on: updatesOn,
      note: updatesOn ? 'Already on' : state.securityUpdates === 'unknown' ? unknown : state.securityUpdates === 'paused' ? 'Paused now' : null,
      preselected: state.securityUpdates === 'disabled' || state.securityUpdates === 'paused',
    },
    {
      key: 'configFile',
      label: 'Add .github/dependabot.yml by pull request',
      description:
        'Opens a pull request on the branch sdi/dependabot with a configuration generated from this repository’s ecosystems. Merging it stays with you.',
      on: state.configPresent,
      note: state.configPresent ? `Already in the repository${state.configPath ? ` at ${state.configPath}` : ''}` : null,
      preselected: !state.configPresent,
    },
  ]
}

/** What the dialog opens with: every option known to be off, ticked. */
export function initialSelection(state: DependabotState): DependabotSelection {
  const selection: DependabotSelection = { alerts: false, securityUpdates: false, configFile: false }
  for (const option of dependabotOptions(state)) selection[option.key] = option.preselected
  return selection
}

/** Nothing is left to turn on: the page offers no button. */
export function isFullyOn(state: DependabotState) {
  return dependabotOptions(state).every((option) => option.on)
}

/** The request for a selection: only what is ticked and not on yet. Null when that is nothing (the route needs at least one). */
export function toRequest(selection: DependabotSelection, state: DependabotState): ConfigureDependabotRequest | null {
  const request: ConfigureDependabotRequest = {}
  for (const option of dependabotOptions(state)) {
    if (selection[option.key] && !option.on) request[option.key] = true
  }
  return Object.keys(request).length > 0 ? request : null
}

/** `POST /projects/{id}/security/dependabot` changes repository settings: admin. */
export function dependabotGate(role: Role): string | null {
  return hasRole(role, 'admin') ? null : 'Turning on Dependabot needs the admin role.'
}
