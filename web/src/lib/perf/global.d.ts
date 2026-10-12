import type { DevPerf } from './dev-perf'

/** Set by `installDevPerf()` in development; read by the API client and the stress page. */
declare global {
  interface Window {
    sdiDevPerf?: DevPerf
  }
}

export {}
