import { CacheControls } from "@/components/cache-controls";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { seriesColor } from "@/lib/history";
import { formatBytes, percent, type Distro, type VmStats } from "@/lib/wsl";

interface Segment {
  key: string;
  label: string;
  bytes: number;
  color: string;
  hint?: string;
}

interface MemoryBreakdownProps {
  vm: VmStats;
  distros: Distro[];
  slots: Map<string, number>;
  onChanged: () => void;
}

/** WSL 虚拟机内存的组成：各发行版私有内存 + 页缓存 + 内核及其他 + 空闲 */
export function MemoryBreakdown({ vm, distros, slots, onChanged }: MemoryBreakdownProps) {
  const distroSegments: Segment[] = distros
    .filter((d) => d.stats)
    .map((d) => ({
      key: d.name,
      label: d.name,
      bytes: d.stats!.memUsed,
      color: seriesColor(slots.get(d.name) ?? 0),
      hint: `进程 RSS 合计 ${formatBytes(d.stats!.memRss)}，其中共享/文件页计入页缓存`,
    }))
    .sort((a, b) => b.bytes - a.bytes);
  const distroTotal = distroSegments.reduce((sum, s) => sum + s.bytes, 0);
  const used = vm.memTotal - vm.memFree;
  const segments: Segment[] = [
    ...distroSegments,
    {
      key: "cache",
      label: "页缓存",
      bytes: vm.memCache,
      color: "var(--mem-cache)",
      hint: "文件读写缓存，可回收。WSL 不会主动把它还给 Windows，可点击“释放缓存”",
    },
    {
      key: "other",
      label: "内核及其他",
      bytes: Math.max(0, used - vm.memCache - distroTotal),
      color: "var(--mem-other)",
      hint: "内核、WSL 系统发行版 (WSLg) 等",
    },
  ];
  const free: Segment = { key: "free", label: "空闲", bytes: vm.memFree, color: "var(--muted)" };

  return (
    <Card className="gap-5">
      <CardHeader className="grid-cols-1! lg:grid-cols-[minmax(0,1fr)_auto]!">
        <CardTitle className="text-sm">内存分布</CardTitle>
        <CardDescription>
          WSL 虚拟机已占用 {formatBytes(used)} / {formatBytes(vm.memTotal)}，其中发行版私有内存{" "}
          {formatBytes(distroTotal)}
        </CardDescription>
        <CardAction className="col-start-1 row-span-1 row-start-3 w-full lg:col-start-2 lg:row-span-2 lg:row-start-1 lg:w-auto">
          <CacheControls onChanged={onChanged} />
        </CardAction>
      </CardHeader>

      <CardContent className="space-y-5">
        {/* 堆叠条：各段之间留 2px 卡片底色的间隙 */}
        <div className="flex h-5 w-full gap-0.5 overflow-hidden rounded-md">
          {[...segments, free]
            .filter((s) => percent(s.bytes, vm.memTotal) >= 0.3)
            .map((s) => (
              <Tooltip key={s.key}>
                <TooltipTrigger asChild>
                  <div
                    className="h-full min-w-0.5 transition-[flex-grow] duration-500 hover:brightness-110"
                    style={{ flex: `${s.bytes} 1 0`, background: s.color }}
                  />
                </TooltipTrigger>
                <TooltipContent>
                  <div className="font-medium tabular-nums">
                    {formatBytes(s.bytes)} · {percent(s.bytes, vm.memTotal).toFixed(1)}%
                  </div>
                  <div className="opacity-80">{s.label}</div>
                </TooltipContent>
              </Tooltip>
            ))}
        </div>

        {/* 明细：同时作为图例和数据表 */}
        <div className="grid gap-x-10 gap-y-2.5 text-sm sm:grid-cols-2">
          {[...segments, free].map((s) => (
            <div key={s.key} className="flex items-center gap-2.5" title={s.hint}>
              <span
                className="size-2.5 shrink-0 rounded-[3px] ring-1 ring-border ring-inset"
                style={{ background: s.color }}
              />
              <span className="truncate text-muted-foreground">{s.label}</span>
              <span className="ml-auto font-medium tabular-nums">{formatBytes(s.bytes)}</span>
              <span className="w-12 text-right text-xs text-muted-foreground tabular-nums">
                {percent(s.bytes, vm.memTotal).toFixed(1)}%
              </span>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
}
