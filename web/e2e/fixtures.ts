/*
 * The `test` every spec uses: its browser context is signed in through the
 * dev login before the page opens, so pages land in the shell instead of on
 * `/login`. Specs that need a signed-out browser create a context of their
 * own (see auth.spec.ts).
 */
import { test as base } from '@playwright/test'

import { signInDev } from './helpers/auth'

export const test = base.extend({
  context: async ({ context }, use) => {
    await signInDev(context.request)
    await use(context)
  },
})

export { expect } from '@playwright/test'
