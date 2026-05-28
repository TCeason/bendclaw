/**
 * Track the last version the user has seen, so we can show
 * "What's New" release notes once after an update.
 */

import { join } from 'path'
import { homedir } from 'os'
import { readFileSync, writeFileSync, mkdirSync } from 'fs'
import { isNewer } from '../update/check.js'

const STATE_DIR = join(homedir(), '.evotai')
const STATE_PATH = join(STATE_DIR, 'last-seen-version.json')

interface State {
  version: string
}

function readState(): State | null {
  try {
    const raw = readFileSync(STATE_PATH, 'utf-8')
    return JSON.parse(raw) as State
  } catch {
    return null
  }
}

function writeState(state: State): void {
  try {
    mkdirSync(STATE_DIR, { recursive: true })
    writeFileSync(STATE_PATH, JSON.stringify(state), 'utf-8')
  } catch { /* best effort */ }
}

/**
 * Check if the current version is newer than what the user last saw.
 * If so, mark it as seen and return true (caller should show release notes).
 * On first install, records the version without triggering notes.
 */
export function shouldShowReleaseNotes(currentVersion: string): boolean {
  const state = readState()

  if (!state) {
    // First install — record version, don't show notes
    writeState({ version: currentVersion })
    return false
  }

  if (isNewer(state.version, currentVersion)) {
    writeState({ version: currentVersion })
    return true
  }

  return false
}
