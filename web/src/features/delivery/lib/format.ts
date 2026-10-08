/** `754` → `12m 34s`; `3600` → `1h`. Always mono + tabular where rendered. */
export function formatDuration(totalSeconds: number) {
  const seconds = Math.max(0, Math.round(totalSeconds))
  if (seconds < 60) return `${seconds}s`

  const minutes = Math.floor(seconds / 60)
  const rest = seconds % 60
  if (minutes < 60) return rest > 0 ? `${minutes}m ${rest}s` : `${minutes}m`

  const hours = Math.floor(minutes / 60)
  const restMinutes = minutes % 60
  return restMinutes > 0 ? `${hours}h ${restMinutes}m` : `${hours}h`
}

export const shortSha = (sha: string) => sha.slice(0, 7)

export const firstLine = (text: string) => text.split('\n', 1)[0] ?? ''

/** 0–1 → 0–100, clamped. */
export const percent = (rate: number) => Math.round(Math.max(0, Math.min(1, rate)) * 100)
