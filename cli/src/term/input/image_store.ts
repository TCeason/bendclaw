/**
 * Image disk store — persist pasted images to ~/.evotai/image-cache/
 * so they survive across history navigation and session restarts.
 *
 * Mirrors claudecode's imageStore.ts approach.
 */
import { mkdir, open } from 'fs/promises'
import { join } from 'path'
import { homedir } from 'os'
import { randomBytes } from 'crypto'

const CACHE_DIR = join(homedir(), '.evotai', 'image-cache')

/** Generate a unique ID for an image paste (not tied to paste ref id). */
function cacheId(): string {
  return randomBytes(8).toString('hex')
}

/**
 * Write image data to disk and return the file path.
 * Images are stored as encoded files based on their media type extension.
 */
export async function storeImage(
  base64: string,
  mediaType: string,
): Promise<string | null> {
  const ext = mediaType.split('/')[1] || 'png'
  const id = cacheId()
  const dir = join(CACHE_DIR, `${id}.${ext}`)
  try {
    await mkdir(CACHE_DIR, { recursive: true })
    const fh = await open(dir, 'w', 0o600)
    try {
      await fh.writeFile(Buffer.from(base64, 'base64'))
      await fh.datasync()
    } finally {
      await fh.close()
    }
    return dir
  } catch (err) {
    return null
  }
}

/**
 * Format an image source annotation for the model.
 * e.g. "[Image #1 source: /Users/.../image-cache/abc.png]"
 */
export function formatImageSourceText(id: number, path: string): string {
  return `[Image #${id} source: ${path}]`
}
