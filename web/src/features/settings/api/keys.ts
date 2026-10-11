/** One document: `GET /settings` composes both services' parts. */
export const settingsKeys = {
  all: ['settings'] as const,
  settings: () => [...settingsKeys.all] as const,
}
