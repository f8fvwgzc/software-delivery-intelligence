/** "3d 4h", "4h 12m", "12m 30s" or "45s". */
export function formatUptime(totalSeconds: number) {
  const seconds = Math.max(0, Math.floor(totalSeconds))
  const days = Math.floor(seconds / 86_400)
  const hours = Math.floor((seconds % 86_400) / 3_600)
  const minutes = Math.floor((seconds % 3_600) / 60)
  const rest = seconds % 60

  if (days > 0) return `${days}d ${hours}h`
  if (hours > 0) return `${hours}h ${minutes}m`
  if (minutes > 0) return `${minutes}m ${rest}s`
  return `${rest}s`
}

/** Mean step/execution duration, same shape as uptime. */
export const formatDuration = formatUptime

export function formatLatency(ms: number) {
  return `${Math.round(ms).toLocaleString()} ms`
}

/** 0–1 → "87%". */
export function formatRate(rate: number) {
  return `${Math.round(Math.max(0, Math.min(1, rate)) * 100)}%`
}

export function formatCount(value: number) {
  return value.toLocaleString()
}
