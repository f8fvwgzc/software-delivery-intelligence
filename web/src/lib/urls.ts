/**
 * Only `http(s)` URLs become links, so nothing like `javascript:` ever
 * reaches an `href`, whether a person typed the value or a service passed
 * it on from somewhere else.
 */
export function safeHttpUrl(value: string | null | undefined): string | undefined {
  if (!value) return undefined
  try {
    const url = new URL(value)
    return url.protocol === 'https:' || url.protocol === 'http:' ? url.href : undefined
  } catch {
    return undefined
  }
}
