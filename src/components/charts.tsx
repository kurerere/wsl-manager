import { Bar, BarChart, CartesianGrid, Cell, LabelList, Line, LineChart, XAxis, YAxis } from "recharts";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  ChartContainer,
  ChartLegend,
  ChartLegendContent,
  ChartTooltip,
  ChartTooltipContent,
  type ChartConfig,
} from "@/components/ui/chart";
import type { Sample } from "@/lib/history";
import { formatBytes } from "@/lib/wsl";

export interface Series {
  key: string;
  label: string;
  color: string;
}

const formatTime = (t: number) =>
  new Date(t).toLocaleTimeString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit", second: "2-digit" });

function toConfig(series: Series[]): ChartConfig {
  return Object.fromEntries(series.map((s) => [s.key, { label: s.label, color: s.color }]));
}

/** 只在每条线的最后一个点画端点，带一圈卡片底色的描边 */
function endDot(lastIndex: number) {
  return function EndDot(props: { cx?: number; cy?: number; index?: number; stroke?: string }) {
    if (props.index !== lastIndex || props.cx == null || props.cy == null) return <g key={props.index} />;
    return (
      <circle
        key={props.index}
        cx={props.cx}
        cy={props.cy}
        r={4}
        fill={props.stroke}
        stroke="var(--card)"
        strokeWidth={2}
      />
    );
  };
}

interface TrendChartProps {
  title: string;
  description: string;
  rows: Sample[];
  series: Series[];
  yMax: number | ((dataMax: number) => number);
  tickFormatter: (v: number) => string;
  valueFormatter: (v: number) => string;
}

export function TrendChart({ title, description, rows, series, yMax, tickFormatter, valueFormatter }: TrendChartProps) {
  const lastIndex = rows.length - 1;
  return (
    <Card className="min-w-0 gap-4">
      <CardHeader>
        <CardTitle className="text-sm">{title}</CardTitle>
        <CardDescription>{description}</CardDescription>
      </CardHeader>
      <CardContent>
        {rows.length < 2 ? (
          <div className="flex h-[220px] items-center justify-center text-sm text-muted-foreground">
            正在收集数据…
          </div>
        ) : (
          <ChartContainer config={toConfig(series)} className="aspect-auto h-[220px] w-full">
            <LineChart data={rows} margin={{ top: 8, right: 8, left: 0, bottom: 0 }}>
              <CartesianGrid vertical={false} />
              <XAxis
                dataKey="t"
                type="number"
                scale="time"
                domain={["dataMin", "dataMax"]}
                tickFormatter={formatTime}
                tickLine={false}
                axisLine={false}
                minTickGap={48}
              />
              <YAxis
                domain={[0, yMax]}
                tickCount={5}
                tickFormatter={tickFormatter}
                tickLine={false}
                axisLine={false}
                width={48}
                allowDataOverflow
              />
              <ChartTooltip
                isAnimationActive={false}
                content={
                  <ChartTooltipContent
                    indicator="line"
                    valueFormatter={valueFormatter}
                    labelFormatter={(_, payload) => formatTime(Number(payload?.[0]?.payload?.t))}
                  />
                }
              />
              <ChartLegend content={<ChartLegendContent iconShape="line" />} />
              {series.map((s) => (
                <Line
                  key={s.key}
                  dataKey={s.key}
                  name={s.key}
                  type="monotone"
                  stroke={`var(--color-${s.key})`}
                  strokeWidth={2}
                  strokeLinecap="round"
                  strokeLinejoin="round"
                  dot={endDot(lastIndex)}
                  activeDot={{ r: 4, stroke: "var(--card)", strokeWidth: 2 }}
                  isAnimationActive={false}
                />
              ))}
            </LineChart>
          </ChartContainer>
        )}
      </CardContent>
    </Card>
  );
}

export interface DiskRow {
  name: string;
  size: number;
  color: string;
}

export function VhdxChart({ rows }: { rows: DiskRow[] }) {
  const sorted = [...rows].sort((a, b) => b.size - a.size);
  return (
    <Card className="min-w-0 gap-4">
      <CardHeader>
        <CardTitle className="text-sm">VHDX 文件占用</CardTitle>
        <CardDescription>各发行版虚拟磁盘在 Windows 上的实际大小</CardDescription>
      </CardHeader>
      <CardContent>
        <ChartContainer config={{ size: { label: "大小" } }} className="aspect-auto h-[220px] w-full">
          <BarChart data={sorted} layout="vertical" margin={{ top: 0, right: 64, left: 0, bottom: 0 }}>
            <XAxis type="number" dataKey="size" hide />
            <YAxis
              type="category"
              dataKey="name"
              tickLine={false}
              axisLine={false}
              width={110}
              tickFormatter={(v: string) => (v.length > 14 ? `${v.slice(0, 13)}…` : v)}
            />
            <ChartTooltip
              cursor={{ fill: "var(--muted)", opacity: 0.5 }}
              isAnimationActive={false}
              content={<ChartTooltipContent hideIndicator valueFormatter={formatBytes} />}
            />
            <Bar dataKey="size" barSize={16} radius={[0, 4, 4, 0]} isAnimationActive={false}>
              {sorted.map((r) => (
                <Cell key={r.name} fill={r.color} />
              ))}
              <LabelList
                dataKey="size"
                position="right"
                className="fill-muted-foreground"
                fontSize={12}
                formatter={(v) => formatBytes(Number(v))}
              />
            </Bar>
          </BarChart>
        </ChartContainer>
      </CardContent>
    </Card>
  );
}
