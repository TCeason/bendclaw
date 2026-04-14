export function shouldAnimateTerminalTitle(): boolean {
  return process.env.EVOT_ANIMATE_TITLE === '1'
}
