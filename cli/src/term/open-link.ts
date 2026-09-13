import { execFile } from 'node:child_process'

/** Launch a validated web link without interpolating it into a shell command. */
export function openWebLink(value: string): Promise<void> {
  const url = new URL(value)
  if (!['https:', 'http:'].includes(url.protocol) || url.username || url.password) {
    return Promise.reject(new Error('Unsupported share URL'))
  }
  const command = process.platform === 'darwin' ? 'open'
    : process.platform === 'win32' ? 'rundll32.exe' : 'xdg-open'
  const args = process.platform === 'win32' ? ['url.dll,FileProtocolHandler', url.href] : [url.href]
  return new Promise((resolve, reject) => {
    execFile(command, args, error => error ? reject(error) : resolve())
  })
}
