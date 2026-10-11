import type { components } from '@/lib/api/schema'

type Proposal = components['schemas']['Proposal']
type ProposalOption = components['schemas']['ProposalOption']

const BUDGET_KEY = 'budget_monthly_usd'
const BUDGET_REQUIREMENT = /^budget[:=]\s*\$?\s*(\d+(?:\.\d+)?)/i
const FIRST_AMOUNT = /\$?\s*(\d+(?:[.,]\d+)?)/

function parseAmount(text: string): number | null {
  const match = FIRST_AMOUNT.exec(text)
  if (!match) return null
  const value = Number.parseFloat(match[1].replace(',', '.'))
  return Number.isFinite(value) && value > 0 ? value : null
}

/**
 * The monthly budget the options are marked against (CONTRACT.md §6), read
 * from the `budget_monthly_usd` constraint. The constraint only carries a
 * description, so the amount is parsed from it; the `budget:<n>` marker on an
 * over-budget option is the fallback.
 */
export function budgetOf(proposal: Pick<Proposal, 'constraints' | 'options'>): number | null {
  const constraint = proposal.constraints.find(
    (candidate) => candidate.key === BUDGET_KEY || candidate.key.startsWith('budget'),
  )
  if (constraint) {
    const fromKey = parseAmount(constraint.key.replace(/^[a-z_]+/i, ''))
    const amount = fromKey ?? parseAmount(constraint.description)
    if (amount !== null) return amount
  }
  for (const option of proposal.options) {
    for (const requirement of option.unmetRequirements) {
      const match = BUDGET_REQUIREMENT.exec(requirement)
      if (match) return Number.parseFloat(match[1])
    }
  }
  return null
}

/** The ranker marks over-budget options with a `budget:<n>` unmet requirement; the estimate is the fallback. */
export function isOverBudget(option: Pick<ProposalOption, 'unmetRequirements' | 'estimatedMonthlyCostUsd'>, budget: number | null) {
  if (option.unmetRequirements.some((requirement) => BUDGET_REQUIREMENT.test(requirement))) return true
  return budget !== null && option.estimatedMonthlyCostUsd > budget
}

/** How far over the budget an option is, in USD per month; 0 when within it. */
export function overBudgetBy(option: Pick<ProposalOption, 'estimatedMonthlyCostUsd'>, budget: number | null) {
  if (budget === null) return 0
  return Math.max(0, option.estimatedMonthlyCostUsd - budget)
}
