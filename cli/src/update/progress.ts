import stripAnsi from 'strip-ansi'

/**
 * Turn curl/wget's terminal progress output into the one progress treatment
 * used by both `evot update` and `/update`.
 *
 * The installer deliberately keeps ownership of the download. This adapter
 * only translates its output after it crosses the process boundary, so a
 * direct `curl | sh` install and an in-app update still use the same installer
 * and report the same percentage.
 */
export function formatInstallerProgress(line: string): string {
  const clean = stripAnsi(line)
    .replace(/[\x00-\x08\x0b-\x1f\x7f-\x9f]/g, '')
    .trim()
  if (!clean) return ''

  // curl --progress-bar emits hash chunks followed by a percentage. The
  // chunks arrive separately because the child-process adapter treats `\r` as
  // a redraw boundary, so suppress them until the percentage is available.
  // This prevents the in-app updater from showing a stream of raw hashes.
  const percentMatch = clean.match(/(?:^|\s)(\d{1,3})(?:\.\d+)?%/)
  let percent = percentMatch ? Number(percentMatch[1]) : Number.NaN

  // Older curl versions use the default meter even when the caller is
  // capturing stderr. Its redraw is the format shown below, with the current
  // percentage in the first column but no literal `%`:
  //
  //   1 36.3M 1 684k 0 0 48481 0 0:13:05 0:00:14 0:12:51 42783
  //
  // Recognize the complete meter shape rather than treating every line that
  // starts with a number as progress output.
  if (!Number.isFinite(percent)) {
    const fields = clean.split(/\s+/)
    const first = Number(fields[0])
    const received = Number(fields[2])
    if (fields.length >= 10 && /^\d{1,3}$/.test(fields[0] ?? '') && /^\d{1,3}$/.test(fields[2] ?? '') && first >= 0 && first <= 100 && received >= 0 && received <= 100) {
      percent = first
    }
  }

  if (!Number.isFinite(percent)) {
    const progressOnly = /^[#=<>*\s]+$/.test(clean)
    return progressOnly ? '' : clean
  }

  // Keep the decimal-free form stable so the status line does not jitter.
  percent = Math.max(0, Math.min(100, percent))
  const width = 24
  const filled = Math.round((width * percent) / 100)
  const bar = '█'.repeat(filled) + '░'.repeat(width - filled)
  return `[${bar}] ${percent}%`
}
