/** One line of a unified diff, typed for display. */
export interface DiffLine {
  kind: 'hunk' | 'add' | 'remove' | 'context' | 'meta'
  /** The line without its leading `+`, `-` or space (hunk and meta lines as they are). */
  text: string
  /** Line number in the live object (`-` side); null where it has none. */
  oldLine: number | null
  /** Line number in the desired object (`+` side). */
  newLine: number | null
}

const HUNK = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/

/**
 * Parses a unified diff (live → desired) into lines with both sides'
 * numbers. The `---`/`+++` file header is dropped: there is one file, the
 * resource. A diff without hunk headers still renders, numbered from 1.
 */
export function parseUnifiedDiff(diff: string): DiffLine[] {
  const lines: DiffLine[] = []
  let oldLine = 1
  let newLine = 1
  for (const raw of diff.replace(/\r\n/g, '\n').replace(/\n$/, '').split('\n')) {
    if (raw.startsWith('--- ') || raw.startsWith('+++ ')) continue
    const hunk = HUNK.exec(raw)
    if (hunk) {
      oldLine = Number(hunk[1])
      newLine = Number(hunk[2])
      lines.push({ kind: 'hunk', text: raw, oldLine: null, newLine: null })
      continue
    }
    if (raw.startsWith('\\')) {
      lines.push({ kind: 'meta', text: raw, oldLine: null, newLine: null })
    } else if (raw.startsWith('+')) {
      lines.push({ kind: 'add', text: raw.slice(1), oldLine: null, newLine: newLine++ })
    } else if (raw.startsWith('-')) {
      lines.push({ kind: 'remove', text: raw.slice(1), oldLine: oldLine++, newLine: null })
    } else {
      lines.push({ kind: 'context', text: raw.startsWith(' ') ? raw.slice(1) : raw, oldLine: oldLine++, newLine: newLine++ })
    }
  }
  return lines
}

/** Added and removed line counts, for the `+3 −1` beside a resource. */
export function diffStats(lines: readonly DiffLine[]): { added: number; removed: number } {
  let added = 0
  let removed = 0
  for (const line of lines) {
    if (line.kind === 'add') added += 1
    else if (line.kind === 'remove') removed += 1
  }
  return { added, removed }
}
