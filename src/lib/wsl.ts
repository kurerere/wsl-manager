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
  vhdxSize: number | null;
  stats: DistroStats | null;
  error: string | null;
}

export interface Overview {
  distros: Distro[];
  vm: VmStats | null;
  host: HostMemory | null;
}

export const api = {
  overview: () => invoke<Overview>("get_overview"),
  terminate: (name: string) => invoke<void>("terminate_distro", { name }),
  dropCaches: () => invoke<void>("drop_caches"),
  shutdown: () => invoke<void>("shutdown_wsl"),
  setDefault: (name: string) => invoke<void>("set_default_distro", { name }),
  openTerminal: (name: string) => invoke<void>("open_terminal", { name }),
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
