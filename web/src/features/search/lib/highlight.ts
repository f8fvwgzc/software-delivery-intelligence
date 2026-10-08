export interface Segment {
  text: string
  match: boolean
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/**
 * Splits `text` into segments, marking the ones that match any word of
 * `query` (case-insensitive). Plain data in, plain data out: the caller
 * renders segments as text nodes, never as HTML.
 */
export function highlight(text: string, query: string): Segment[] {
  const terms = query
    .split(/\s+/)
    .map((term) => term.trim())
    .filter((term) => term.length >= 2)

  if (terms.length === 0 || text.length === 0) return [{ text, match: false }]

  const pattern = new RegExp(`(${terms.map(escapeRegExp).join('|')})`, 'gi')
  const segments: Segment[] = []
  let last = 0

  for (const found of text.matchAll(pattern)) {
    const index = found.index
    if (index > last) segments.push({ text: text.slice(last, index), match: false })
    segments.push({ text: found[0], match: true })
    last = index + found[0].length
  }
  if (last < text.length) segments.push({ text: text.slice(last), match: false })

  return segments
}
