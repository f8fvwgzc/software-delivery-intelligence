const usd = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  maximumFractionDigits: 0,
})

const usdExact = new Intl.NumberFormat('en-US', {
  style: 'currency',
  currency: 'USD',
  minimumFractionDigits: 2,
  maximumFractionDigits: 2,
})

/** "$120/mo", or "Free" for a zero estimate. */
export function formatMonthlyCost(value: number) {
  if (value <= 0) return 'Free'
  return `${usd.format(value)}/mo`
}

/** "$12" for whole amounts, "$12.50" otherwise. Used where lines must visibly add up. */
export function formatUsd(value: number) {
  return Number.isInteger(value) ? usd.format(value) : usdExact.format(value)
}

/** 0–1 score as a whole percentage string. */
export function formatPercent(value: number) {
  return `${Math.round(Math.max(0, Math.min(1, value)) * 100)}%`
}
