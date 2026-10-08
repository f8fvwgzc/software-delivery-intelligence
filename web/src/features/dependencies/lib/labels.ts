const ECOSYSTEM: Record<string, string> = {
  npm: 'npm',
  pnpm: 'pnpm',
  yarn: 'Yarn',
  cargo: 'Cargo',
  pip: 'pip',
  poetry: 'Poetry',
  go: 'Go modules',
  maven: 'Maven',
  gradle: 'Gradle',
  composer: 'Composer',
  bundler: 'Bundler',
  mix: 'Mix',
  nuget: 'NuGet',
  docker: 'Docker',
  github_actions: 'GitHub Actions',
  terraform: 'Terraform',
  helm: 'Helm',
}

/** Display name for an ecosystem key; unknown keys are shown as they are. */
export function ecosystemLabel(key: string) {
  return ECOSYSTEM[key] ?? key
}

const SCOPE: Record<string, string> = {
  runtime: 'Runtime',
  dev: 'Dev',
  build: 'Build',
}

export function scopeLabel(key: string) {
  return SCOPE[key] ?? key
}

/** Sentinel for "every ecosystem" in the filter select (Radix reserves the empty string). */
export const ALL_ECOSYSTEMS = 'all'
