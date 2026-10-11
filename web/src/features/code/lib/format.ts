const UNITS = ['B', 'KB', 'MB', 'GB'] as const

/** `1234` → `1.2 KB`. Whole bytes stay whole. */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  let value = bytes
  let unit = 0
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024
    unit++
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${UNITS[unit]}`
}

export function shortSha(sha: string): string {
  return sha.slice(0, 7)
}

const LANGUAGE_LABEL: Record<string, string> = {
  typescript: 'TypeScript',
  tsx: 'TSX',
  javascript: 'JavaScript',
  jsx: 'JSX',
  rust: 'Rust',
  python: 'Python',
  go: 'Go',
  markdown: 'Markdown',
  json: 'JSON',
  yaml: 'YAML',
  toml: 'TOML',
  dockerfile: 'Dockerfile',
  shell: 'Shell',
  makefile: 'Makefile',
  sql: 'SQL',
  css: 'CSS',
  html: 'HTML',
  ignore: 'Ignore rules',
}

/** Display name for the analyzer's lowercase language id. */
export function languageLabel(language: string): string {
  if (!language) return 'Plain text'
  return LANGUAGE_LABEL[language] ?? language.charAt(0).toUpperCase() + language.slice(1)
}

export function isMarkdown(language: string, name: string): boolean {
  return language === 'markdown' || /\.(md|markdown)$/i.test(name)
}

/** Lines of a file for rendering; a trailing newline does not add an empty last line. */
export function splitLines(content: string): string[] {
  if (content === '') return []
  const lines = content.split('\n')
  if (lines[lines.length - 1] === '') lines.pop()
  return lines
}

/** `#L42` → 42; anything else → null. */
export function parseLineHash(hash: string): number | null {
  const match = /^#L(\d+)$/.exec(hash)
  if (!match) return null
  const line = Number(match[1])
  return Number.isInteger(line) && line > 0 ? line : null
}
