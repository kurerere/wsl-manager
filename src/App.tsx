import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { Cpu, HardDrive, MemoryStick, PowerOff, RefreshCw } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { TrendChart, VhdxChart, type Series } from "@/components/charts";
import { DistroCard } from "@/components/distro-card";
import { CacheControls } from "@/components/cache-controls";
import { MemoryBreakdown } from "@/components/memory-breakdown";
import { Metric } from "@/components/metric";
import { appendSample, emptyHistory, seriesColor, seriesKey, VM_KEY, type History } from "@/lib/history";
import { api, formatBytes, percent, type Overview } from "@/lib/wsl";
import { cn } from "@/lib/utils";

const INTERVALS = [
  { value: "2000", label: "每 2 秒" },
  { value: "5000", label: "每 5 秒" },
  { value: "10000", label: "每 10 秒" },
  { value: "0", label: "暂停刷新" },
];

export default function App() {
  const [data, setData] = useState<Overview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [interval, setIntervalMs] = useState("5000");
  const [history, setHistory] = useState<History>(emptyHistory);
  const inFlight = useRef(false);
  // 发行版名 -> 颜色槽位，首次出现时分配后不再变化
  const slots = useRef(new Map<string, number>());

  const refresh = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setLoading(true);
    try {
      const next = await api.overview();
      for (const d of next.distros) {
        if (!slots.current.has(d.name)) slots.current.set(d.name, slots.current.size);
      }
      setData(next);
      setHistory((h) => appendSample(h, next, slots.current));
      setError(null);
    } catch (e) {
      setError(String(e));
    } finally {
      inFlight.current = false;
      setLoading(false);
    }
  }, []);

  // 上一次采样结束后再计时，避免请求堆积；窗口最小化/不可见时暂停，可见后立即刷新
  useEffect(() => {
    let timer: number | undefined;
    let cancelled = false;
    const tick = async () => {
      window.clearTimeout(timer);
      if (document.hidden) return;
      await refresh();
      if (cancelled || interval === "0") return;
      window.clearTimeout(timer);
      timer = window.setTimeout(tick, Number(interval));
    };
    const onVisibility = () => {
      if (!document.hidden && interval !== "0") tick();
    };
    tick();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [interval, refresh]);

  const shutdownAll = async () => {
    try {
      await api.shutdown();
    } catch (e) {
      setError(String(e));
    }
    refresh();
  };

  const distros = [...(data?.distros ?? [])].sort((a, b) => Number(b.running) - Number(a.running));
  const runningCount = distros.filter((d) => d.running).length;
  const vm = data?.vm;
  const host = data?.host;
  const vhdxTotal = distros.reduce((sum, d) => sum + (d.vhdxSize ?? 0), 0);

  // 只为在历史窗口内有过数据的发行版画线，按 wsl 列表顺序排列
  const seen = new Set(history.cpu.flatMap((r) => Object.keys(r).filter((k) => r[k] != null)));
  const series: Series[] = [
    { key: VM_KEY, label: "虚拟机总计", color: "var(--muted-foreground)" },
    ...(data?.distros ?? []).flatMap((d) => {
      const slot = slots.current.get(d.name) ?? 0;
      const key = seriesKey(slot);
      return seen.has(key) ? [{ key, label: d.name, color: seriesColor(slot) }] : [];
    }),
  ];
  const memTotalGiB = vm ? vm.memTotal / 1024 ** 3 : 0;

  return (
    <div className="min-h-screen">
      <header className="sticky top-0 z-10 border-b bg-background/80 backdrop-blur">
        <div className="mx-auto flex max-w-7xl items-center gap-3 px-6 py-3">
          <img src="/app-icon.svg" alt="" className="size-8" draggable={false} />
          <div className="mr-auto">
            <h1 className="font-semibold leading-tight">WSL Manager</h1>
            <p className="text-xs text-muted-foreground">
              {data ? `${distros.length} 个发行版 · ${runningCount} 个运行中` : "正在读取…"}
            </p>
          </div>
          <Select value={interval} onValueChange={setIntervalMs}>
            <SelectTrigger size="sm" className="w-28">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {INTERVALS.map((i) => (
                <SelectItem key={i.value} value={i.value}>
                  {i.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button variant="outline" size="sm" onClick={refresh} disabled={loading}>
            <RefreshCw className={cn(loading && "animate-spin")} />
            刷新
          </Button>
          <Button variant="outline" size="sm" onClick={shutdownAll} disabled={runningCount === 0}>
            <PowerOff />
            全部关闭
          </Button>
        </div>
      </header>

      <main className="mx-auto max-w-7xl space-y-6 px-6 py-6">
        {error && (
          <Alert variant="destructive">
            <AlertTitle>操作失败</AlertTitle>
            <AlertDescription className="select-text whitespace-pre-wrap">{error}</AlertDescription>
          </Alert>
        )}

        <section className="grid gap-4 md:grid-cols-3">
          <SummaryCard icon={<Cpu className="size-4" />} title="WSL 虚拟机 CPU">
            {vm ? <Metric label={`${vm.cpuCores} 核`} value={vm.cpuPercent} /> : <EmptySummary loading={!data} />}
          </SummaryCard>
          <SummaryCard icon={<MemoryStick className="size-4" />} title="Windows 物理内存">
            {host ? (
              <Metric
                label={vm ? `WSL 约占 ${formatBytes(vm.memTotal - vm.memFree)}` : "已用"}
                value={percent(host.total - host.available, host.total)}
                detail={`${formatBytes(host.total - host.available)} / ${formatBytes(host.total)}`}
              />
            ) : (
              <EmptySummary loading={!data} />
            )}
          </SummaryCard>
          <SummaryCard icon={<HardDrive className="size-4" />} title="VHDX 磁盘占用">
            {data ? (
              <div className="flex items-baseline justify-between text-sm">
                <span className="text-muted-foreground">{distros.length} 个虚拟磁盘</span>
                <span className="text-xl font-semibold tabular-nums">{formatBytes(vhdxTotal)}</span>
              </div>
            ) : (
              <EmptySummary loading />
            )}
          </SummaryCard>
        </section>

        {!vm && (
          <Card>
            <CardContent className="flex flex-wrap items-center justify-between gap-4">
              <p className="text-sm text-muted-foreground">
                {data ? "没有运行中的 WSL 虚拟机" : "正在读取 WSL 状态…"}
              </p>
              <CacheControls onChanged={refresh} />
            </CardContent>
          </Card>
        )}

        {vm && (
          <MemoryBreakdown
            vm={vm}
            distros={distros}
            slots={slots.current}
            onChanged={refresh}
          />
        )}

        <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          <TrendChart
            title="CPU 使用率"
            description="占 WSL 虚拟机 CPU 总容量的比例"
            rows={history.cpu}
            series={series}
            yMax={(max) => Math.min(100, Math.max(20, Math.ceil(max / 20) * 20))}
            tickFormatter={(v) => `${v}%`}
            valueFormatter={(v) => `${v.toFixed(1)}%`}
          />
          <TrendChart
            title="内存占用"
            description={vm ? `各发行版私有内存，虚拟机共 ${formatBytes(vm.memTotal)}` : "各发行版私有内存"}
            rows={history.mem}
            series={series}
            yMax={(max) => Math.max(1, Math.ceil(Math.max(max, memTotalGiB)))}
            tickFormatter={(v) => `${v} GB`}
            valueFormatter={(v) => formatBytes(v * 1024 ** 3)}
          />
          {data && (
            <div className="min-w-0 md:col-span-2 xl:col-span-1">
              <VhdxChart
                rows={distros
                  .filter((d) => d.vhdxSize != null)
                  .map((d) => ({
                    name: d.name,
                    size: d.vhdxSize ?? 0,
                    color: seriesColor(slots.current.get(d.name) ?? 0),
                  }))}
              />
            </div>
          )}
        </section>

        <section className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
          {!data && Array.from({ length: 3 }, (_, i) => <Skeleton key={i} className="h-64 rounded-xl" />)}
          {distros.map((d) => (
            <DistroCard
              key={d.name}
              distro={d}
              vmMemTotal={vm?.memTotal ?? 0}
              onChanged={refresh}
              onError={setError}
            />
          ))}
          {data && distros.length === 0 && (
            <p className="col-span-full py-12 text-center text-muted-foreground">未找到已安装的 WSL 发行版</p>
          )}
        </section>

        <p className="text-xs text-muted-foreground">
          说明：WSL2 的所有发行版运行在同一个虚拟机中。单个发行版的 CPU 为其进程占整个虚拟机 CPU
          容量的比例（两次刷新之间的平均值），内存为其进程的私有内存之和（不含共享内存和文件缓存），磁盘为发行版根分区的使用情况。
        </p>
      </main>
    </div>
  );
}

function SummaryCard({ icon, title, children }: { icon: ReactNode; title: string; children: ReactNode }) {
  return (
    <Card className="gap-3">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-sm font-medium text-muted-foreground">
          {icon}
          {title}
        </CardTitle>
      </CardHeader>
      <CardContent>{children}</CardContent>
    </Card>
  );
}

function EmptySummary({ loading }: { loading: boolean }) {
  return loading ? <Skeleton className="h-6" /> : <p className="text-sm text-muted-foreground">没有运行中的发行版</p>;
}
