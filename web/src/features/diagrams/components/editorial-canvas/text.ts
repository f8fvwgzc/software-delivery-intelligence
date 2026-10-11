/**
 * Text measurement for layout. Widths come from a 2D canvas with the
 * project's own fonts, so labels are never clipped by a box sized from a
 * guess; the per-character budget of the diagram-design style guide (0.60 em
 * sans, 0.62 em mono) is the fallback when there is no document.
 */

export interface Font {
  size: number
  weight?: number
  mono?: boolean
}

const SANS_EM = 0.6
const MONO_EM = 0.62
/** Fonts may still be swapping in when the first layout runs; the slack keeps a late Geist from overflowing. */
const SLACK = 1.04

let context: CanvasRenderingContext2D | null | undefined
let families: { sans: string; mono: string } | undefined
const cache = new Map<string, number>()

function fontString(font: Font): string {
  if (!families) {
    const style = getComputedStyle(document.documentElement)
    families = {
      sans: style.getPropertyValue('--font-sans').trim() || 'sans-serif',
      mono: style.getPropertyValue('--font-mono').trim() || 'monospace',
    }
  }
  return `${font.weight ?? 400} ${font.size}px ${font.mono ? families.mono : families.sans}`
}

/** Width of `text` in px at `font`. */
export function textWidth(text: string, font: Font): number {
  if (text.length === 0) return 0
  if (typeof document === 'undefined') return text.length * font.size * (font.mono ? MONO_EM : SANS_EM)
  if (context === undefined) context = document.createElement('canvas').getContext('2d')
  if (!context) return text.length * font.size * (font.mono ? MONO_EM : SANS_EM)
  const key = `${font.size}/${font.weight ?? 400}/${font.mono ? 'm' : 's'}/${text}`
  let width = cache.get(key)
  if (width === undefined) {
    context.font = fontString(font)
    width = context.measureText(text).width * SLACK
    cache.set(key, width)
  }
  return width
}

/** Cuts `text` to what fits in `width`, with an ellipsis. */
export function fitText(text: string, width: number, font: Font): string {
  if (textWidth(text, font) <= width) return text
  let low = 0
  let high = text.length
  while (low < high) {
    const mid = Math.ceil((low + high) / 2)
    if (textWidth(`${text.slice(0, mid).trimEnd()}…`, font) <= width) low = mid
    else high = mid - 1
  }
  return `${text.slice(0, Math.max(low, 1)).trimEnd()}…`
}

/** Points a label may break at: after a space, hyphen, underscore, slash or dot. */
const BREAKS = /(?<=[ \-_/.])/

/**
 * Wraps `text` into at most `maxLines` lines of at most `width` px; the
 * last line is cut with an ellipsis when the text is longer still.
 */
export function wrapText(text: string, width: number, font: Font, maxLines = 2): string[] {
  const tokens = text.split(BREAKS).filter((token) => token.length > 0)
  const lines: string[] = []
  let current = ''
  let index = 0
  for (; index < tokens.length; index++) {
    const candidate = current + tokens[index]
    if (current.length === 0 || textWidth(candidate.trimEnd(), font) <= width) {
      current = candidate
      continue
    }
    // No room for another line: the rest is cut below.
    if (lines.length === maxLines - 1) break
    lines.push(current.trimEnd())
    current = tokens[index]
  }
  // Whatever was not placed (a single over-long token, or the tail) is the last line, cut to fit.
  lines.push((current + tokens.slice(index).join('')).trimEnd())
  return lines.map((line) => fitText(line, width, font))
}

/** Widest line of a wrapped label. */
export function linesWidth(lines: readonly string[], font: Font): number {
  return lines.reduce((max, line) => Math.max(max, textWidth(line, font)), 0)
}
