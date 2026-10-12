import { queryOptions } from '@tanstack/react-query'

import { api, unwrap } from '@/lib/api/client'
import type { components } from '@/lib/api/schema'

/** The public face of a user: login, name, avatar. Never an email (CONTRACT.md §18). */
export type Person = components['schemas']['Person']

const USER_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i

/**
 * Rows store who acted as a user id, or as a plain word for automation
 * (`system`, a service name). Only ids are looked up; words are shown as
 * they are.
 */
export function isUserId(value: string) {
  return USER_ID.test(value)
}

/** The route's cap on ids per call. */
const BATCH_LIMIT = 100
/** A login or avatar changes rarely; a row re-rendering must not refetch it. */
const NAMES_STALE_MS = 10 * 60_000

interface Waiter {
  resolve: (person: Person | null) => void
  reject: (error: unknown) => void
}

let waiting = new Map<string, Waiter[]>()
let scheduled = false

/** One request names everyone the rows of this render asked about. */
async function flush() {
  scheduled = false
  const batch = waiting
  waiting = new Map()
  const ids = [...batch.keys()]

  for (let start = 0; start < ids.length; start += BATCH_LIMIT) {
    const chunk = ids.slice(start, start + BATCH_LIMIT)
    try {
      const people = await unwrap(api.GET('/api/v1/people', { params: { query: { ids: chunk.join(',') } } }))
      const byId = new Map(people.map((person) => [person.id, person]))
      // An id the workspace may not name is absent from the answer: that is `null`, not a failure.
      for (const id of chunk) for (const waiter of batch.get(id) ?? []) waiter.resolve(byId.get(id) ?? null)
    } catch (error) {
      for (const id of chunk) for (const waiter of batch.get(id) ?? []) waiter.reject(error)
    }
  }
}

function load(id: string): Promise<Person | null> {
  return new Promise((resolve, reject) => {
    const waiters = waiting.get(id)
    if (waiters) waiters.push({ resolve, reject })
    else waiting.set(id, [{ resolve, reject }])
    if (!scheduled) {
      scheduled = true
      // After the commit that mounted the rows: every row of a table lands in the same request.
      queueMicrotask(() => void flush())
    }
  })
}

export const peopleKeys = {
  all: ['people'] as const,
  person: (id: string) => [...peopleKeys.all, id.toLowerCase()] as const,
}

export const peopleQueries = {
  /**
   * Who a stored user id is. Cached per id, fetched in batches, and scoped
   * to the current workspace by the server (the cache is reset on a
   * workspace switch like every other query).
   */
  person: (id: string) =>
    queryOptions({
      queryKey: peopleKeys.person(id),
      queryFn: () => load(id.toLowerCase()),
      staleTime: NAMES_STALE_MS,
      gcTime: 30 * 60_000,
    }),
}
