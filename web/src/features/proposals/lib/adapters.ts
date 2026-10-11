import type { components } from '@/lib/api/schema'

export type Adapter = components['schemas']['Adapter']

export interface AdapterChoice {
  value: Adapter
  label: string
  description: string
}

/** How an accepted plan can be run (CONTRACT.md §5.4), in the order offered. */
export const ADAPTER_CHOICES: readonly AdapterChoice[] = [
  {
    value: 'simulated',
    label: 'Dry run (simulated)',
    description: 'Walks every step through its gates with timed, simulated results. Touches nothing.',
  },
  {
    value: 'docker',
    label: 'Verify locally with Docker',
    description: 'Builds and runs the image on this machine, needs the local-docker identity.',
  },
  {
    value: 'github',
    label: 'Open pull request',
    description: 'Commits the generated files to a new branch and opens a PR, needs repository write.',
  },
]
