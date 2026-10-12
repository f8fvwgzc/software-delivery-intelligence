/** Upper snake case, as the API asks for workspace secret names: `JENKINS_TOKEN` (CONTRACT.md §22). */
export const SECRET_NAME_PATTERN = /^[A-Z][A-Z0-9_]*$/

export const SECRET_NAME_HINT = 'Upper case letters, digits and underscores, starting with a letter.'

/** What was typed, bent towards a valid name: `jenkins token` → `JENKINS_TOKEN`. */
export function normalizeSecretName(typed: string) {
  return typed
    .toUpperCase()
    .replace(/[\s-]+/g, '_')
    .replace(/[^A-Z0-9_]/g, '')
}
