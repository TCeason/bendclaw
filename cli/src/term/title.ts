export class TerminalTitle {
  private frozen = false

  constructor(
    private readonly cwd: string,
    private readonly port: () => number | null,
  ) {}

  set(suffix?: string, force = false): void {
    if (this.frozen && !force) return
    this.write(suffix)
  }

  freeze(suffix?: string): void {
    this.write(suffix)
    this.frozen = true
  }

  unfreeze(): void {
    this.frozen = false
  }

  private write(suffix?: string): void {
    const dirName = this.cwd.split('/').pop() || this.cwd
    const base = `evot - ${dirName}`
    const port = this.port()
    const portPart = port ? ` · :${port}` : ''
    const title = suffix ? `${suffix} ${base}${portPart}` : `${base}${portPart}`
    process.stdout.write(`\x1b]0;${title}\x07`)
  }
}
