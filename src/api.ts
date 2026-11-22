import { invoke } from '@tauri-apps/api/core'

export type MountState = 'mounted' | 'unmounted' | 'error'

export interface RemoteSpec {
  name: string
  url: string
  vendor?: string | null
  user: string
  mountPoint: string
  subPath: string
  extraFlags: string[]
  autoMount: boolean
  networkMode: boolean
}

export interface MountStatus {
  name: string
  state: MountState
  pid?: number | null
  mountPoint: string
  error?: string | null
}

export interface SettingsView {
  remotes: RemoteSpec[]
  statuses: MountStatus[]
  rcloneVersion: string
  rclonePath: string
  rcloneFound: boolean
  winfspOk: boolean
  winfspRequired: boolean
  autostart: boolean
  autoRemount: boolean
  dataDir: string
  platform: string
}

export const api = {
  settings: () => invoke<SettingsView>('get_settings'),
  saveRemote: (input: { spec: RemoteSpec; pass?: string | null }) =>
    invoke<RemoteSpec>('save_remote', { input }),
  deleteRemote: (name: string) => invoke<void>('delete_remote', { name }),
  mount: (name: string) => invoke<MountStatus>('mount_remote', { name }),
  unmount: (name: string) => invoke<MountStatus>('unmount_remote', { name }),
  statuses: () => invoke<MountStatus[]>('list_statuses'),
  logTail: (name: string, lines = 60) => invoke<string>('read_log', { name, lines }),
  setAutoRemount: (enabled: boolean) => invoke<SettingsView>('set_auto_remount', { enabled }),
  setAutostart: (enabled: boolean) => invoke<SettingsView>('set_autostart', { enabled }),
  openDataDir: () => invoke<void>('open_data_dir'),
  hideWindow: () => invoke<void>('hide_window'),
}

export function errorText(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return JSON.stringify(e)
}