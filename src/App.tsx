import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { Cpu, Download, HardDrive, MemoryStick, PowerOff, RefreshCw, Settings } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuCheckboxItem,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Progress } from "@/components/ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { TrendChart, VhdxChart, type Series } from "@/components/charts";
import { DistroCard } from "@/components/distro-card";
import { CacheControls } from "@/components/cache-controls";
import { MemoryBreakdown } from "@/components/memory-breakdown";
import { Metric } from "@/components/metric";
import { appendSample, emptyHistory, seriesColor, seriesKey, VM_KEY, type History } from "@/lib/history";
import {
  api,
  formatBytes,
  percent,
  type AppSettings,
  type Overview,
  type UpdateProgress,
  type UpdateStatus,
} from "@/lib/wsl";
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
  const [appSettings, setAppSettings] = useState<AppSettings | null>(null);
  const [settingsBusy, setSettingsBusy] = useState(false);
  const [windowVisible, setWindowVisible] = useState(false);
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
  const [updateDialogOpen, setUpdateDialogOpen] = useState(false);
  const [updateDialogError, setUpdateDialogError] = useState<string | null>(null);
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateDownloading, setUpdateDownloading] = useState(false);
  const [updateInstalling, setUpdateInstalling] = useState(false);
  const [updateProgress, setUpdateProgress] = useState<UpdateProgress | null>(null);
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

  useEffect(() => {
    void api.getAppSettings().then(setAppSettings).catch((e) => setError(String(e)));
  }, []);

  useEffect(() => {
    let cancelled = false;
    let receivedEvent = false;
    const subscription = listen<boolean>("window-visibility", ({ payload }) => {
      receivedEvent = true;
      setWindowVisible(payload);
    });
    void subscription.catch(() => {});
    void api.windowVisibility().then((visible) => {
      if (!cancelled && !receivedEvent) setWindowVisible(visible);
    }).catch((e) => {
      if (!cancelled) {
        setError(String(e));
        setWindowVisible(!document.hidden);
      }
    });
    return () => {
      cancelled = true;
      void subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    let receivedEvent = false;
    let unlisten: (() => void)[] = [];
    const subscriptions = Promise.all([
      listen<UpdateStatus>("update-available", ({ payload }) => {
        receivedEvent = true;
        setUpdateStatus(payload);
      }),
      listen<UpdateProgress>("update-progress", ({ payload }) => setUpdateProgress(payload)),
      listen("update-installing", () => {
        setUpdateDownloading(false);
        setUpdateInstalling(true);
      }),
    ]);
    void subscriptions.then(async (listeners) => {
      if (cancelled) {
        listeners.forEach((stop) => stop());
        return;
      }
      unlisten = listeners;
      try {
        const status = await api.updateStatus();
        if (!cancelled && !receivedEvent && status) setUpdateStatus(status);
      } catch {}
    }).catch(() => {});
    return () => {
      cancelled = true;
      unlisten.forEach((stop) => stop());
    };
  }, []);

  const updateSetting = async (
    setter: (enabled: boolean) => Promise<AppSettings>,
    enabled: boolean,
  ) => {
    setSettingsBusy(true);
    try {
      setAppSettings(await setter(enabled));
    } catch (e) {
      setError(String(e));
    } finally {
      setSettingsBusy(false);
    }
  };

  const checkForUpdates = async () => {
    setUpdateDialogOpen(true);
    setUpdateDialogError(null);
    setUpdateChecking(true);
    try {
      setUpdateStatus(await api.checkUpdate());
    } catch (e) {
      setUpdateDialogError(String(e));
    } finally {
      setUpdateChecking(false);
    }
  };

  const installUpdate = async () => {
    setUpdateDialogError(null);
    setUpdateProgress({ downloaded: 0, total: null });
    setUpdateDownloading(true);
    try {
      await api.installUpdate();
    } catch (e) {
      setUpdateDialogError(String(e));
      setUpdateDownloading(false);
      setUpdateInstalling(false);
    }
  };

  const openReleasePage = async () => {
    try {
      await api.openReleasePage();
      setUpdateDialogOpen(false);
    } catch (e) {
      setUpdateDialogError(String(e));
    }
  };

  // 上一次采样结束后再计时，避免请求堆积；窗口最小化/不可见时暂停，可见后立即刷新
  useEffect(() => {
    let timer: number | undefined;
    let cancelled = false;
    const tick = async () => {
      window.clearTimeout(timer);
      if (!windowVisible || document.hidden) return;
      await refresh();
      if (cancelled || interval === "0") return;
      window.clearTimeout(timer);
      timer = window.setTimeout(tick, Number(interval));
    };
    const onVisibility = () => {
      if (windowVisible && !document.hidden && interval !== "0") tick();
    };
    tick();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [interval, refresh, windowVisible]);

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
          {updateStatus?.available && (
            <Button variant="outline" size="sm" onClick={() => setUpdateDialogOpen(true)}>
              <Download data-icon="inline-start" />
              新版本 v{updateStatus.available.version}
            </Button>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="sm" aria-label="设置">
                <Settings data-icon="inline-start" />
                设置
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuGroup>
                <DropdownMenuCheckboxItem
                  checked={appSettings?.autostart ?? false}
                  disabled={!appSettings || settingsBusy}
                  onCheckedChange={(enabled) => void updateSetting(api.setAutostart, enabled)}
                >
                  开机自启
                </DropdownMenuCheckboxItem>
                <DropdownMenuCheckboxItem
                  checked={appSettings?.closeToTray ?? true}
                  disabled={!appSettings || settingsBusy}
                  onCheckedChange={(enabled) => void updateSetting(api.setCloseToTray, enabled)}
                >
                  关闭窗口时最小化到托盘
                </DropdownMenuCheckboxItem>
              </DropdownMenuGroup>
              <DropdownMenuSeparator />
              <DropdownMenuGroup>
                <DropdownMenuItem disabled={updateChecking} onSelect={() => void checkForUpdates()}>
                  <Download />
                  检查更新
                </DropdownMenuItem>
              </DropdownMenuGroup>
            </DropdownMenuContent>
          </DropdownMenu>
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
      <AlertDialog
        open={updateDialogOpen}
        onOpenChange={(open) => {
          if (!updateChecking && !updateDownloading && !updateInstalling) setUpdateDialogOpen(open);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>
              {updateStatus?.available
                ? `发现新版本 v${updateStatus.available.version}`
                : updateDialogError
                  ? "检查更新失败"
                  : "检查更新"}
            </AlertDialogTitle>
            {updateStatus?.available ? (
              <>
                <AlertDialogDescription>当前版本 v{updateStatus.currentVersion}</AlertDialogDescription>
                <div className="max-h-60 overflow-y-auto rounded-md border p-3 text-sm whitespace-pre-wrap">
                  {updateStatus.available.notes || "暂无更新说明"}
                </div>
              </>
            ) : (
              <AlertDialogDescription>
                {updateDialogError ??
                  (updateStatus
                    ? `已是最新版本 v${updateStatus.currentVersion}`
                    : "正在检查更新…")}
              </AlertDialogDescription>
            )}
            {updateDialogError && updateStatus?.available && (
              <p className="text-sm text-destructive whitespace-pre-wrap">{updateDialogError}</p>
            )}
          </AlertDialogHeader>
          {updateDownloading && updateProgress && (
            <div className="flex flex-col gap-2">
              <Progress
                value={
                  updateProgress.total
                    ? Math.min(100, (updateProgress.downloaded / updateProgress.total) * 100)
                    : 40
                }
                indicatorClassName={!updateProgress.total ? "animate-pulse" : undefined}
                aria-label="更新下载进度"
              />
              <p className="text-xs text-muted-foreground">
                {updateProgress.total
                  ? `已下载 ${formatBytes(updateProgress.downloaded)} / ${formatBytes(updateProgress.total)}`
                  : `已下载 ${formatBytes(updateProgress.downloaded)}（总大小未知）`}
              </p>
            </div>
          )}
          {updateInstalling && (
            <p role="status" className="text-sm text-muted-foreground">正在安装，应用将自动退出</p>
          )}
          <AlertDialogFooter>
            <AlertDialogCancel disabled={updateChecking || updateDownloading || updateInstalling}>
              取消
            </AlertDialogCancel>
            {updateStatus?.available && updateStatus.installable && (
              <AlertDialogAction
                disabled={updateChecking || updateDownloading || updateInstalling}
                onClick={(event) => {
                  event.preventDefault();
                  void installUpdate();
                }}
              >
                {updateDownloading ? "正在下载…" : "下载并安装"}
              </AlertDialogAction>
            )}
            {updateStatus?.available && !updateStatus.installable && (
              <AlertDialogAction
                disabled={updateChecking || updateDownloading || updateInstalling}
                onClick={(event) => {
                  event.preventDefault();
                  void openReleasePage();
                }}
              >
                前往下载
              </AlertDialogAction>
            )}
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
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
