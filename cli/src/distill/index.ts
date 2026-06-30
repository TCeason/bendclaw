/** Public entrypoint for the distill subsystem.
 *
 * Keep CLI-facing imports here so the rest of the application does not depend
 * on internal pipeline modules directly. Implementation details live under
 * ./internal/ and are imported directly only by focused unit tests.
 */

export { runDistill } from './cli.js'
export type { DistillOptions, DomainSpec, TaskSpec } from './internal/types.js'
