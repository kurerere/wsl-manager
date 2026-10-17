import { useState } from "react";
import { HardDrive, MoreVertical, Power, Star, TerminalSquare } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { Metric } from "@/components/metric";
import { api, formatBytes, percent, type Distro } from "@/lib/wsl";
import { cn } from "@/lib/utils";

interface DistroCardProps {
  distro: Distro;
  vmMemTotal: number;
  onChanged: () => void;
  onError: (message: string) => void;
}

export function DistroCard({ distro, vmMemTotal, onChanged, onError }: DistroCardProps) {
  const [busy, setBusy] = useState(false);
  const { name, running, stats } = distro;

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
      onChanged();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className={cn(!running && "opacity-75")}>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <span
            className={cn("size-2 shrink-0 rounded-full", running ? "bg-metric-ok" : "bg-muted-foreground/40")}
          />
          <span className="truncate">{name}</span>
          {distro.isDefault && (
            <Badge variant="secondary" className="gap-1">
              <Star className="size-3" />
              默认
            </Badge>
          )}
        </CardTitle>
        <CardDescription className="flex items-center gap-2">
          <span>WSL {distro.version}</span>
          <span>·</span>
          <span>{running ? "运行中" : "已停止"}</span>
          {stats && (
            <>
              <span>·</span>
              <span>{stats.processCount} 个进程</span>
            </>
          )}
        </CardDescription>
        <CardAction>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="ghost" size="icon" className="size-8" disabled={busy}>
                <MoreVertical />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => run(() => api.openTerminal(name))}>
                <TerminalSquare />
                打开终端
              </DropdownMenuItem>
              <DropdownMenuItem disabled={distro.isDefault} onClick={() => run(() => api.setDefault(name))}>
                <Star />
                设为默认
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                variant="destructive"
                disabled={!running}
                onClick={() => run(() => api.terminate(name))}
              >
                <Power />
                终止
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </CardAction>
      </CardHeader>

      <CardContent className="space-y-4">
        {running && stats ? (
          <>
            <Metric label="CPU" value={stats.cpuPercent} />
            <Metric
              label="内存（私有）"
              value={percent(stats.memUsed, vmMemTotal)}
              detail={`${formatBytes(stats.memUsed)} / ${formatBytes(vmMemTotal)}`}
            />
            <Metric
              label="磁盘"
              value={percent(stats.diskUsed, stats.diskTotal)}
              detail={`${formatBytes(stats.diskUsed)} / ${formatBytes(stats.diskTotal)}`}
            />
          </>
        ) : running && distro.error ? (
          <p className="h-[124px] text-sm text-destructive">采样失败：{distro.error}</p>
        ) : running ? (
          <div className="space-y-4">
            <Skeleton className="h-6" />
            <Skeleton className="h-6" />
            <Skeleton className="h-6" />
          </div>
        ) : (
          <div className="flex h-[124px] flex-col items-center justify-center gap-3 text-sm text-muted-foreground">
            <span>发行版未运行</span>
            <Button size="sm" variant="outline" disabled={busy} onClick={() => run(() => api.openTerminal(name))}>
              <TerminalSquare />
              启动并打开终端
            </Button>
          </div>
        )}

        {distro.vhdxSize != null && (
          <Tooltip>
            <TooltipTrigger asChild>
              <div className="flex items-center gap-1.5 border-t pt-3 text-xs text-muted-foreground">
                <HardDrive className="size-3.5" />
                <span>VHDX 文件占用</span>
                <span className="ml-auto tabular-nums">{formatBytes(distro.vhdxSize)}</span>
              </div>
            </TooltipTrigger>
            <TooltipContent className="select-text">{distro.vhdxPath}</TooltipContent>
          </Tooltip>
        )}
      </CardContent>
    </Card>
  );
}
