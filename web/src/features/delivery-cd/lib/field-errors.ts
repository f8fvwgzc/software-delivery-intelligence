import { ApiError, isNetworkError } from '@/lib/api/errors'

/** A form field and the words a server message may use for it. */
export interface FieldAlias {
  /** The form's own key for the field, e.g. `healthPath` or `config.workflow`. */
  field: string
  /** Names the services use: `health_path`, `config.workflow`, `workflow`. */
  aliases: readonly string[]
}

export interface SaveError {
  /** Set when the message names exactly one field: it is shown under that field. */
  field: string | null
  message: string
}

const escape = (value: string) => value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')

/** The alias as a whole word: `url` must not match inside `curl`. */
const mentions = (message: string, alias: string) => new RegExp(`(^|[^a-z0-9_.])${escape(alias)}($|[^a-z0-9_])`, 'i').test(message)

/** `name: already used` → [`name`, `already used`]; `config.workflow is required` → [`config.workflow`, null]. */
function leadingField(message: string): { name: string; rest: string | null } | null {
  const match = /^([a-z][a-z0-9_.]*)(:\s*|\s+)(.*)$/is.exec(message)
  if (!match) return null
  return { name: match[1], rest: match[2].startsWith(':') ? match[3] : null }
}

const capitalize = (text: string) => text.charAt(0).toUpperCase() + text.slice(1)

/**
 * The gateway answers a refused save with `{ code, message }`, and the
 * message leads with the field it is about: `config.workflow must end in
 * .yml`, `name: an environment called staging exists`, `commit_sha: not
 * found in the repository` (CONTRACT.md §22). This finds the form field, so
 * the message sits where it can be fixed; a `field:` prefix is dropped
 * there, since the label above already says it. A message that leads with
 * no known field falls back to the fields it mentions: the most specific
 * one wins, and one that fits two equally, or none, stays on the form as a
 * whole.
 */
export function toSaveError(error: unknown, fields: readonly FieldAlias[]): SaveError {
  if (isNetworkError(error)) {
    return { field: null, message: 'The server did not answer. Nothing was saved; try again in a moment.' }
  }
  const message = error instanceof Error ? error.message : 'The change could not be saved.'
  // Only a refusal of the input belongs to a field; a missing role or a lost session does not.
  if (!(error instanceof ApiError) || ![400, 409, 412, 422].includes(error.status)) return { field: null, message }

  const leading = leadingField(message)
  if (leading) {
    const named = fields.find(({ aliases }) => aliases.some((alias) => alias.toLowerCase() === leading.name.toLowerCase()))
    if (named) return { field: named.field, message: leading.rest ? capitalize(leading.rest) : message }
  }

  let best: { field: string; length: number } | null = null
  let tied = false
  for (const { field, aliases } of fields) {
    for (const alias of aliases) {
      if (!mentions(message, alias)) continue
      if (!best || alias.length > best.length) {
        best = { field, length: alias.length }
        tied = false
      } else if (alias.length === best.length && field !== best.field) {
        tied = true
      }
    }
  }
  return { field: best && !tied ? best.field : null, message }
}

/** `healthPath` → [`healthPath`, `health_path`, `health path`]. */
export function aliasesOf(field: string): string[] {
  const snake = field.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`)
  return [...new Set([field, snake, snake.replaceAll('_', ' ')])]
}
