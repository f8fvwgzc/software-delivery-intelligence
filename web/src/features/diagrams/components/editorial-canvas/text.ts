/** Per-character width budget (em) from the diagram-design style guide: 0.60 sans, 0.62 mono. */
const SANS_EM = 0.6
const MONO_EM = 0.62

/** Cuts a label to what fits in `width` px at `fontSize`, with an ellipsis. */
export function fitText(text: string, width: number, fontSize: number, mono = false): string {
  const perChar = fontSize * (mono ? MONO_EM : SANS_EM)
  const max = Math.max(3, Math.floor(width / perChar))
  if (text.length <= max) return text
  return `${text.slice(0, max - 1).trimEnd()}…`
}

/** Width a run of text is budgeted to take, for pills and masks. */
export function textWidth(text: string, fontSize: number, mono = false): number {
  return text.length * fontSize * (mono ? MONO_EM : SANS_EM)
}
