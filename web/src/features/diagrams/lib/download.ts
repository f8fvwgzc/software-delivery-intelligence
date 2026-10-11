/** Saves a blob through a transient anchor. */
export function downloadBlob(blob: Blob, fileName: string) {
  const url = URL.createObjectURL(blob)
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = fileName
  anchor.click()
  // Revoke after the click has been dispatched; Safari needs a tick.
  window.setTimeout(() => URL.revokeObjectURL(url), 1000)
}
