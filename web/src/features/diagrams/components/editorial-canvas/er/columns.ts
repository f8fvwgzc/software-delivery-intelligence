export interface Column {
  name: string
  type: string
  pk: boolean
  /** `table.column` the foreign key points at, when the line says so. */
  fk: string | null
  notNull: boolean
  unique: boolean
}

export interface ParsedColumns {
  columns: Column[]
  /** Columns the builder left out ("(+11 more)"). */
  more: number
  /** The column count the caption states ("SQLite · 17 columns"), when there is one. */
  count: number | null
}

const PK = /\bPK\b/i
const FK = /\bFK\b\s*(?:→|->)?\s*([\w.]+)?/i
const NOT_NULL = /\bNOT\s+NULL\b/i
const UNIQUE = /\bUNIQUE\b/i
const MORE = /\(\s*\+\s*(\d+)\s+more\s*\)/i
const COUNT = /(\d+)\s+columns?/i

function parseLine(line: string): Column {
  let rest = line
  const pk = PK.test(rest)
  rest = rest.replace(PK, '')
  const fkMatch = rest.match(FK)
  const fk = fkMatch ? fkMatch[1] ?? '' : null
  rest = rest.replace(FK, '')
  const notNull = NOT_NULL.test(rest)
  rest = rest.replace(NOT_NULL, '')
  const unique = UNIQUE.test(rest)
  rest = rest.replace(UNIQUE, '')
  const [name = '', ...type] = rest.replace(/[[\]]/g, ' ').trim().split(/\s+/)
  return { name, type: type.join(' '), pk, fk, notNull, unique }
}

/**
 * Columns from a table node's description: one per line as
 * "name type [PK] [FK → table.col] [NOT NULL] [UNIQUE]". A single
 * comma-separated line of names (the builder's short form) is read as
 * names only, with its "(+N more)" tail.
 */
export function parseColumns(description: string | undefined, caption: string | undefined): ParsedColumns {
  const countMatch = caption?.match(COUNT)
  const count = countMatch ? Number(countMatch[1]) : null
  const text = description?.trim() ?? ''
  if (text.length === 0) return { columns: [], more: 0, count }

  const lines = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line.length > 0)

  if (lines.length === 1 && lines[0].includes(',') && !/\s/.test(lines[0].split(',')[0].trim())) {
    const moreMatch = lines[0].match(MORE)
    const names = lines[0]
      .replace(MORE, '')
      .split(',')
      .map((name) => name.trim())
      .filter((name) => name.length > 0)
    return {
      columns: names.map((name) => ({ name, type: '', pk: false, fk: null, notNull: false, unique: false })),
      more: moreMatch ? Number(moreMatch[1]) : 0,
      count,
    }
  }

  const columns: Column[] = []
  let more = 0
  for (const line of lines) {
    const moreMatch = line.match(MORE)
    if (moreMatch) {
      more += Number(moreMatch[1])
      continue
    }
    const column = parseLine(line)
    if (column.name) columns.push(column)
  }
  return { columns, more, count }
}
