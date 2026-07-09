/**
 * Fence-boundary repair for model-glued / stray / unclosed code fences.
 *
 * Architecture:
 *   signals.ts  — pure detectors ("what is this line?")
 *   cases.ts    — named repair rules (match + apply); only business decisions
 *   engine.ts   — thin line scanner that schedules cases
 *
 * CommonMark only recognizes fences at line start. Models frequently:
 *   1. Glue an opening fence to the end of a heading/prose line
 *   2. Emit a bare ``` that was meant as a close (swallows the rest as code)
 *   3. Leave a fence unclosed before the next heading/paragraph
 *
 * This package fixes only those fence-boundary failures. It deliberately
 * does NOT restore the broader glue-normalization (tables/headings/hr/…)
 * that was dropped to stay closer to pi's "render as-is" philosophy.
 *
 * New bug workflow: classify → add fixture under tests/fixtures/fences/<id>/
 * → add/adjust one case in cases.ts → keep engine untouched.
 */

export { prepareMarkdownFences, repairUnclosedFences, splitGluedFenceOpens } from './engine.js'
export { CASES, matchFenceCase } from './cases.js'
export type { FenceAction, FenceCase, FenceCtx, FenceOpenState } from './types.js'
