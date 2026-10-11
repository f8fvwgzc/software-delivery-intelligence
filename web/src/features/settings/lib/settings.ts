import type {
  IntelligenceSettings,
  IntelligenceSettingsPatch,
  KnowledgeSettings,
  KnowledgeSettingsPatch,
  ScoreWeights,
  Settings,
  UpdateSettingsRequest,
} from '../api/queries'

/* Scanner */

export type ScannerNumberKey = keyof Omit<KnowledgeSettings, 'cacheGithub'>

export interface ScannerField {
  key: ScannerNumberKey
  label: string
  description: string
  min: number
  step: number
}

export const SCANNER_FIELDS: readonly ScannerField[] = [
  { key: 'scanMaxFiles', label: 'Max files', description: 'Files analysed per scan.', min: 1, step: 100 },
  { key: 'scanMaxFileBytes', label: 'Max file bytes', description: 'Largest file whose content is read.', min: 1, step: 1024 },
  { key: 'scanConcurrency', label: 'Concurrency', description: 'Files fetched at the same time.', min: 1, step: 1 },
  { key: 'pipelineRunsLimit', label: 'Pipeline runs limit', description: 'Runs synced from the provider per refresh.', min: 1, step: 10 },
]

/* Ranking */

export type WeightKey = keyof ScoreWeights

export const WEIGHT_FIELDS: readonly { key: WeightKey; label: string }[] = [
  { key: 'architectureFit', label: 'Architecture fit' },
  { key: 'semanticSimilarity', label: 'Semantic similarity' },
  { key: 'success', label: 'Success rate' },
  { key: 'security', label: 'Security' },
  { key: 'cost', label: 'Cost' },
  { key: 'policy', label: 'Policy agreement' },
  { key: 'recency', label: 'Recency' },
]

export const REASONERS = ['heuristic', 'claude'] as const
export type Reasoner = (typeof REASONERS)[number]

/** Floating point sums of seven weights are never exactly 1. */
const SUM_TOLERANCE = 0.0005

export function sumWeights(weights: Record<WeightKey, number>) {
  return WEIGHT_FIELDS.reduce((total, field) => total + weights[field.key], 0)
}

export function weightsSumToOne(weights: Record<WeightKey, number>) {
  return Math.abs(sumWeights(weights) - 1) <= SUM_TOLERANCE
}

/** Every weight is a finite number in 0–1. */
export function weightsInRange(weights: Record<WeightKey, number>) {
  return WEIGHT_FIELDS.every((field) => {
    const value = weights[field.key]
    return Number.isFinite(value) && value >= 0 && value <= 1
  })
}

/** Zeroed weights for a server that has none configured yet. */
export const EMPTY_WEIGHTS: ScoreWeights = {
  architectureFit: 0,
  cost: 0,
  policy: 0,
  recency: 0,
  security: 0,
  semanticSimilarity: 0,
  success: 0,
}

/** The reasoner that actually answers, given the configured one and the key. */
export function effectiveReasoner(settings: Pick<IntelligenceSettings, 'reasoner' | 'apiKeyConfigured'>) {
  return settings.reasoner === 'claude' && !settings.apiKeyConfigured ? 'heuristic' : settings.reasoner
}

/* Optimistic merge */

function mergeKnowledge(current: KnowledgeSettings, patch: KnowledgeSettingsPatch): KnowledgeSettings {
  return {
    cacheGithub: patch.cacheGithub ?? current.cacheGithub,
    pipelineRunsLimit: patch.pipelineRunsLimit ?? current.pipelineRunsLimit,
    scanConcurrency: patch.scanConcurrency ?? current.scanConcurrency,
    scanMaxFileBytes: patch.scanMaxFileBytes ?? current.scanMaxFileBytes,
    scanMaxFiles: patch.scanMaxFiles ?? current.scanMaxFiles,
  }
}

function mergeIntelligence(current: IntelligenceSettings, patch: IntelligenceSettingsPatch): IntelligenceSettings {
  return {
    apiKeyConfigured: current.apiKeyConfigured,
    costCeilingUsd: patch.costCeilingUsd ?? current.costCeilingUsd,
    defaultWeights: patch.defaultWeights ?? current.defaultWeights,
    model: patch.model ?? current.model,
    reasoner: patch.reasoner ?? current.reasoner,
  }
}

/** What the server will answer once the patch lands: unset fields keep their value. */
export function applySettingsPatch(current: Settings, patch: UpdateSettingsRequest): Settings {
  return {
    knowledge: patch.knowledge ? mergeKnowledge(current.knowledge, patch.knowledge) : current.knowledge,
    intelligence: patch.intelligence
      ? mergeIntelligence(current.intelligence, patch.intelligence)
      : current.intelligence,
  }
}
