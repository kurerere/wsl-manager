import { invoke } from "@tauri-apps/api/core";

export interface DistroStats {
  cpuPercent: number;
  /** 私有内存（RSS 减去共享/文件页） */
  memUsed: number;
  memRss: number;
  processCount: number;
  diskTotal: number;
  diskUsed: number;
  diskAvail: number;
}

export interface VmStats {
  cpuPercent: number;
  cpuCores: number;
  memTotal: number;
  memAvailable: number;
  memFree: number;
  memCache: number;
}

export interface HostMemory {
  total: number;
  available: number;
}

export interface Distro {
  name: string;
  running: boolean;
  version: number;
  isDefault: boolean;
  vhdxPath: string | null;
  compactBlocker: string | null;
  vhdxSize: number | null;
  stats: DistroStats | null;
  error: string | null;
}

export interface Overview {
  distros: Distro[];
  vm: VmStats | null;
  host: HostMemory | null;
}

export interface CacheSchedule {
  minutes: number;
  remaining: number;
  busy: boolean;
  waiting: boolean;
  completed: number;
  error: string | null;
}

export interface AppSettings {
  autostart: boolean;
  closeToTray: boolean;
}

export interface CompactResult {
  before: number;
  after: number;
}

export const api = {
  overview: () => invoke<Overview>("get_overview"),
  terminate: (name: string) => invoke<void>("terminate_distro", { name }),
  compactDistro: (name: string) => invoke<CompactResult>("compact_distro", { name }),
  dropCaches: () => invoke<void>("drop_caches"),
  cacheSchedule: () => invoke<CacheSchedule>("get_cache_schedule"),
  setCacheSchedule: (minutes: number) => invoke<CacheSchedule>("set_cache_schedule", { minutes }),
  shutdown: () => invoke<void>("shutdown_wsl"),
  setDefault: (name: string) => invoke<void>("set_default_distro", { name }),
  openTerminal: (name: string) => invoke<void>("open_terminal", { name }),
  getAppSettings: () => invoke<AppSettings>("get_app_settings"),
  setAutostart: (enabled: boolean) => invoke<AppSettings>("set_autostart", { enabled }),
  setCloseToTray: (enabled: boolean) => invoke<AppSettings>("set_close_to_tray", { enabled }),
  windowVisibility: () => invoke<boolean>("get_window_visibility"),
};

export function formatBytes(bytes: number): string {
  if (!bytes) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const i = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const v = bytes / 1024 ** i;
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

export function percent(part: number, total: number): number {
  return total > 0 ? (part / total) * 100 : 0;
}
