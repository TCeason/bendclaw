// Helper for overriding `process.stdout.columns` in tests.
//
// On newer Bun versions, when stdout is not a TTY (as in CI), `columns` is a
// read-only getter and a direct `process.stdout.columns = N` assignment throws
// `TypeError: Attempted to assign to readonly property`. Using
// `Object.defineProperty` works regardless of Bun version and TTY state.
//
// Returns a restore function that puts the original descriptor back.
export function withColumns(columns: number): () => void {
  const original = Object.getOwnPropertyDescriptor(process.stdout, 'columns')

  Object.defineProperty(process.stdout, 'columns', {
    value: columns,
    configurable: true,
    writable: true,
    enumerable: true,
  })

  return () => {
    if (original) {
      Object.defineProperty(process.stdout, 'columns', original)
    } else {
      // No own descriptor existed (value came from the prototype getter).
      // Delete the override so reads fall back to the original behavior.
      delete (process.stdout as { columns?: number }).columns
    }
  }
}
