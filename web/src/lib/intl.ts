/**
 * Locale-aware formatting through `Intl`, so no component concatenates a
 * plural suffix or a thousands separator by hand. The locale is the
 * browser's; the formatters are built once.
 */

const locale = typeof navigator === 'undefined' ? 'en-US' : navigator.language

const integer = new Intl.NumberFormat(locale)
const compact = new Intl.NumberFormat(locale, { notation: 'compact', maximumFractionDigits: 1 })
const plurals = new Intl.PluralRules(locale)
const dateTime = new Intl.DateTimeFormat(locale, { dateStyle: 'medium', timeStyle: 'medium' })
const clock = new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23' })
const shortDateTime = new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit', hourCycle: 'h23' })

/** `1234` → `1,234` (or `1.234`, by locale). */
export function formatNumber(value: number): string {
  return integer.format(value)
}

/** `12345` → `12.3K`. */
export function formatCompact(value: number): string {
  return compact.format(value)
}

/**
 * `count(3, 'file', 'files')` → `3 files`; `count(1, 'file', 'files')` →
 * `1 file`. The plural category comes from `Intl.PluralRules`, so a locale
 * with more than two forms still picks the right branch for `one`.
 */
export function count(value: number, one: string, other: string): string {
  return `${formatNumber(value)} ${plurals.select(value) === 'one' ? one : other}`
}

/** The word alone: `plural(1, 'file', 'files')` → `file`. */
export function plural(value: number, one: string, other: string): string {
  return plurals.select(value) === 'one' ? one : other
}

/** `2026-10-12T14:05:09Z` → `Oct 12, 2026, 2:05:09 PM`; the input unchanged when it is not a date. */
export function formatDateTime(iso: string): string {
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? iso : dateTime.format(date)
}

/** `Oct 12, 14:05:09`: the audit table's dense form; the input unchanged when it is not a date. */
export function formatShortDateTime(iso: string): string {
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? iso : shortDateTime.format(date)
}

/** `HH:mm:ss` of an ISO timestamp, or `--:--:--` when it does not parse. */
export function formatClock(iso: string): string {
  const date = new Date(iso)
  return Number.isNaN(date.getTime()) ? '--:--:--' : clock.format(date)
}
