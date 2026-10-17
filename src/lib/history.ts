import type { Overview } from "@/lib/wsl";

/** 每个指标保留的采样点数 */
export const MAX_SAMPLES = 60;
export const VM_KEY = "vm";
const GIB = 1024 ** 3;

export type Sample = { t: number } & Record<string, number | null>;

export interface History {
  cpu: Sample[];
  /** 单位 GiB，便于坐标轴取整 */
  mem: Sample[];
}

export const emptyHistory: History = { cpu: [], mem: [] };

/** 发行版在图表里的系列 key，按首次出现顺序固定分配，颜色随发行版走而不随排序变化 */
export function seriesKey(slot: number) {
  return `s${slot}`;
}

/** 分类色只有 8 个，超出的发行版用中性灰，避免颜色循环造成混淆 */
export function seriesColor(slot: number) {
  return slot < 8 ? `var(--series-${slot + 1})` : "var(--muted-foreground)";
}

export function appendSample(history: History, data: Overview, slots: Map<string, number>): History {
  const t = Date.now();
  const cpu: Sample = { t, [VM_KEY]: data.vm?.cpuPercent ?? null };
  const mem: Sample = { t, [VM_KEY]: data.vm ? (data.vm.memTotal - data.vm.memFree) / GIB : null };
  for (const d of data.distros) {
    const key = seriesKey(slots.get(d.name) ?? 0);
    cpu[key] = d.stats?.cpuPercent ?? null;
    mem[key] = d.stats ? d.stats.memUsed / GIB : null;
  }
  return {
    cpu: [...history.cpu, cpu].slice(-MAX_SAMPLES),
    mem: [...history.mem, mem].slice(-MAX_SAMPLES),
  };
}
