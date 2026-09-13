import { relativeTime } from '../../render/format.js'
import { selectorExpandItems, selectorRemoveItem, type SelectorState } from '../selector.js'
import { createAppSelectorState } from './selector-identity.js'

export interface SharedSession {
  id: string
  url: string
  title?: string | null
  created_at?: number | null
  size_bytes?: number | null
}

export function shareSelectorState(shares: SharedSession[]): SelectorState {
  const state = createAppSelectorState('shares', 'Shared sessions', shares.map(share => ({
    id: share.id,
    label: share.id.slice(0, 8),
    detail: [share.title || '(untitled)', age(share.created_at)].filter(Boolean).join('  ·  '),
    searchText: `${share.title ?? ''} ${share.id} ${share.url}`,
    preview: [share.title || '(untitled)', share.url, '',
      ...created(share.created_at), ...size(share.size_bytes),
      'Anyone with the link can read this snapshot.',
      'Deleting revokes this link, not the local session.'],
    hints: [
      { keys: ['up', 'down'], action: 'move' },
      { keys: 'enter', action: 'open' },
      { keys: 'ctrl+d', action: 'delete' },
      { keys: 'escape', action: 'close' },
    ],
  })))
  return { ...state, subtitle: shares.length ? undefined : 'No shared sessions yet.' }
}

export interface ShareSelectorDeps {
  list(): Promise<SharedSession[]>
  delete(id: string): Promise<void>
  open(url: string): Promise<void>
  current(): SelectorState | undefined
  publish(state: SelectorState): void
}

/** Each opening is a separate controller: stale requests cannot replace another list. */
export class ShareSelector {
  private shares: SharedSession[] = []
  private deleting = new Set<string>()

  constructor(private readonly deps: ShareSelectorDeps) {}

  async load(): Promise<void> {
    const loading = { ...shareSelectorState([]), subtitle: 'Loading shared sessions…' }
    this.deps.publish(loading)
    try {
      this.shares = await this.deps.list()
      const current = this.deps.current()
      if (current) {
        const loaded = shareSelectorState(this.shares)
        this.deps.publish({ ...selectorExpandItems(current, loaded.allItems), subtitle: loaded.subtitle })
      }
    } catch (error) {
      const current = this.deps.current()
      if (current) this.deps.publish({ ...current, subtitle: message(error) })
    }
  }

  async open(id: string): Promise<void> {
    const share = this.shares.find(share => share.id === id)
    if (!share) return
    try { await this.deps.open(share.url) } catch (error) {
      const current = this.deps.current()
      if (current) this.deps.publish({ ...current, subtitle: message(error) })
    }
  }

  async delete(id: string): Promise<void> {
    if (this.deleting.has(id)) return
    this.deleting.add(id)
    try {
      await this.deps.delete(id)
      this.shares = this.shares.filter(share => share.id !== id)
      const current = this.deps.current()
      if (current) {
        const index = current.items.findIndex(item => item.id === id)
        // Remove by id even when a search/movement occurred during the request.
        const pool = current.allItems.filter(item => item.id !== id)
        const next = index >= 0 ? selectorRemoveItem(current, index) : { ...current, allItems: pool }
        this.deps.publish({ ...next, allItems: pool, pendingDeleteId: undefined, subtitle: 'Share deleted.' })
      }
    } catch (error) {
      const current = this.deps.current()
      if (current) this.deps.publish({ ...current, pendingDeleteId: undefined, subtitle: message(error) })
    } finally { this.deleting.delete(id) }
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function age(createdAt: number | null | undefined): string {
  return typeof createdAt === 'number' ? relativeTime(new Date(createdAt).toISOString()) : ''
}

function created(createdAt: number | null | undefined): string[] {
  return typeof createdAt === 'number' ? [`Created: ${new Date(createdAt).toLocaleString()}`] : []
}

function size(sizeBytes: number | null | undefined): string[] {
  return typeof sizeBytes === 'number' ? [`Size: ${(sizeBytes / 1024).toFixed(1)} KiB`] : []
}
