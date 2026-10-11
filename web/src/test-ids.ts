/**
 * The one map of `data-testid` values. Tests prefer roles, labels and text;
 * an id is added only where the element has no role of its own (a callout,
 * a definition term, a list item) or the text would be ambiguous. Both the
 * components and the Playwright specs (`web/e2e`) import from here, so a
 * rename cannot leave the two out of step.
 */
export const testIds = {
  understanding: {
    /** The TL;DR callout at the top of the understanding page. */
    tldr: 'understanding-tldr',
    /** One detected fact (`dt`) under "What was detected". */
    detectedFact: 'understanding-detected-fact',
    /** The live stage rail shown while a scan runs. */
    scanRail: 'understanding-scan-rail',
    /** The delivery maturity score (CONTRACT.md §13), wherever a page renders it. */
    maturityScore: 'understanding-maturity-score',
  },
  proposal: {
    /** One agreed constraint in the negotiation panel. */
    constraint: 'proposal-constraint',
  },
  execution: {
    /** The paused step expanded in place: cause, raw output, remediation. */
    failurePanel: 'execution-failure-panel',
  },
  policies: {
    /** The locked decision cell of a policy that cannot be relaxed. */
    lockedDecision: 'policy-locked-decision',
  },
} as const
