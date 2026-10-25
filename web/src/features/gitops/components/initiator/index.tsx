import { Person } from '@/components/shared/data/person'

import { SYSTEM_INITIATOR } from '../../lib/labels'

/** Who started an operation: a person by login and avatar, or the sync policy for an automated sync. Never a raw id. */
export function Initiator({ value, className }: { value: string | null | undefined; className?: string }) {
  if (value?.trim() === SYSTEM_INITIATOR) return <span className={className}>Sync policy</span>
  return <Person value={value} className={className} />
}
