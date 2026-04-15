import React from 'react'

export interface SystemMsg {
  level: 'info' | 'warn' | 'error'
  text: string
}

export function pushSystem(
  setter: React.Dispatch<React.SetStateAction<SystemMsg[]>>,
  level: SystemMsg['level'],
  text: string,
) {
  setter((prev) => [...prev, { level, text }])
}
