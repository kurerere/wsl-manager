import { useCallback, useEffect, useRef, useState } from "react";
import { Eraser, Loader2 } from "lucide-react";
import { Popover } from "radix-ui";
import { listen } from "@tauri-apps/api/event";
import { Button } from "@/components/ui/button";
import { api, type CacheSchedule } from "@/lib/wsl";
import { cn } from "@/lib/utils";

export function CacheControls({ onChanged }: { onChanged: () => void }) {
  const [status, setStatus] = useState<CacheSchedule | null>(null);
  const [pending, setPending] = useState(false);
  const [manual, setManual] = useState(false);
  const [open, setOpen] = useState(false);
  const [custom, setCustom] = useState("45");
  const [error, setError] = useState<string | null>(null);
  const [connectionError, setConnectionError] = useState<string | null>(null);
  const [success, setSuccess] = useState(false);
  const completed = useRef<number | null>(null);
  const request = useRef(0);

  const accept = useCallback((next: CacheSchedule) => {
    setStatus(next);
    if (completed.current !== null && next.completed !== completed.current) {
      setSuccess(!next.error);
      onChanged();
    }
    completed.current = next.completed;
  }, [onChanged]);

  useEffect(() => {
    let cancelled = false;
    const subscription = listen<CacheSchedule>("cache-schedule", ({ payload }) => {
      if (!cancelled) {
        ++request.current;
        accept(payload);
      }
    });
    // 事件不可用时仍由状态轮询恢复界面。
    void subscription.catch(() => {});
    return () => {
      cancelled = true;
      void subscription.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [accept]);

  useEffect(() => {
    let cancelled = false;
    let timer: number;
    const poll = async () => {
      const id = ++request.current;
      try {
        const next = await api.cacheSchedule();
        if (!cancelled && id === request.current) {
          accept(next);
          setConnectionError(null);
        }
      } catch (e) {
        if (!cancelled) setConnectionError(`读取定时状态失败：${String(e)}`);
      }
      if (!cancelled) timer = window.setTimeout(poll, 1000);
    };
    void poll();
    return () => { cancelled = true; window.clearTimeout(timer); };
  }, [accept]);

  useEffect(() => {
    if (!success) return;
    const timer = window.setTimeout(() => setSuccess(false), 4000);
    return () => window.clearTimeout(timer);
  }, [success]);

  const configure = async (minutes: number) => {
    ++request.current;
    setPending(true);
    setError(null);
    try {
      const next = await api.setCacheSchedule(minutes);
      ++request.current;
      accept(next);
      setOpen(false);
    } catch (e) { setError(String(e)); }
    finally { setPending(false); }
  };

  const release = async () => {
    setManual(true);
    setError(null);
    setSuccess(false);
    try {
      await api.dropCaches();
      ++request.current;
      accept(await api.cacheSchedule());
    } catch (e) { setError(String(e)); }
    finally { setManual(false); }
  };

  const busy = manual || status?.busy;
  const minutes = status?.minutes ?? 0;
  const customSelected = minutes > 0 && minutes !== 15 && minutes !== 30;
  const remaining = status?.remaining ?? 0;
  const countdown = `${Math.floor(remaining / 60).toString().padStart(2, "0")}:${(remaining % 60).toString().padStart(2, "0")}`;
  const segment = (selected: boolean) => cn(
    "rounded-none shadow-none focus-visible:z-10",
    selected ? "bg-accent text-accent-foreground" : "text-muted-foreground",
  );

  return (
    <div className="flex max-w-full flex-col items-end gap-2">
      <div className="flex flex-wrap items-center justify-end gap-2">
        <div role="group" aria-label="定时释放周期"
          className="inline-flex rounded-md shadow-xs [&>button+button]:-ml-px [&>button:first-child]:rounded-l-md [&>button:last-child]:rounded-r-md">
          {[15, 30].map((value) => (
            <Button key={value} variant="outline" size="sm" className={segment(minutes === value)} aria-pressed={minutes === value}
              disabled={!status || pending} onClick={() => void configure(value)}>{value} 分钟</Button>
          ))}
          <Popover.Root open={open} onOpenChange={setOpen}>
            <Popover.Trigger asChild>
              <Button variant="outline" size="sm" className={segment(customSelected)} aria-pressed={customSelected}
                disabled={!status || pending} onClick={() => setCustom(customSelected ? String(minutes) : custom)}>
                {customSelected ? `${minutes} 分钟` : "自定义"}
              </Button>
            </Popover.Trigger>
            <Popover.Portal>
              <Popover.Content align="end" sideOffset={8} collisionPadding={16} className="z-50 w-72 rounded-md border bg-popover p-4 text-popover-foreground shadow-md">
                <form onSubmit={(event) => { event.preventDefault(); void configure(Number(custom)); }} className="space-y-3">
                  <label htmlFor="cache-minutes" className="text-sm font-medium">自定义释放间隔</label>
                  <div className="flex items-center gap-2">
                    <input id="cache-minutes" type="number" min="1" max="1440" step="1" required value={custom}
                      onChange={(event) => setCustom(event.target.value)} aria-describedby="cache-minutes-hint"
                      className="h-9 min-w-0 flex-1 rounded-md border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring" />
                    <span className="text-sm">分钟</span>
                  </div>
                  <p id="cache-minutes-hint" className="text-xs text-muted-foreground">1～1440 分钟。仅在应用运行时生效。</p>
                  <div className="flex justify-end gap-2">
                    <Button type="button" variant="outline" size="sm" onClick={() => setOpen(false)}>取消</Button>
                    <Button type="submit" size="sm" disabled={pending}>{pending ? "保存中…" : "确认"}</Button>
                  </div>
                </form>
              </Popover.Content>
            </Popover.Portal>
          </Popover.Root>
          <Button variant="outline" size="sm" className={segment(minutes === 0)} aria-pressed={minutes === 0}
            disabled={!status || pending} onClick={() => void configure(0)}>关闭</Button>
        </div>
        <Button variant="outline" size="sm" className={cn("tabular-nums", (minutes > 0 || status?.waiting) && "w-44")} onClick={() => void release()}
          disabled={!status || busy || status.waiting || !!connectionError} aria-busy={busy}
          title="释放 Linux 文件缓存；之后首次读取文件可能稍慢。定时释放仅在应用运行时生效。">
          {busy ? <Loader2 className="animate-spin motion-reduce:animate-none" /> : <Eraser />}
          {busy ? "正在释放…" : status?.waiting ? "等待 WSL 运行" : <>释放缓存{minutes > 0 && <span className="text-muted-foreground">· {countdown}</span>}</>}
        </Button>
      </div>
      {(connectionError || error || status?.error) && <p role="alert" className="max-w-md text-xs text-destructive">{connectionError || error || `释放失败：${status?.error}`}</p>}
      {success && !error && !status?.error && <p role="status" className="text-xs text-muted-foreground">缓存已释放</p>}
    </div>
  );
}
