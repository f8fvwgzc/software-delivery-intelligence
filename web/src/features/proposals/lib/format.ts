const usd = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  maximumFractionDigits: 0,
})

/** "$120/mo", or "Free" for a zero estimate. */
export function formatMonthlyCost(value: number) {
  if (value <= 0) return 'Free'
  return `${usd.format(value)}/mo`
}

/** 0–1 score as a whole percentage string. */
export function formatPercent(value: number) {
  return `${Math.round(Math.max(0, Math.min(1, value)) * 100)}%`
}
