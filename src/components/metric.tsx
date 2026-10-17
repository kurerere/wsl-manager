import { Progress } from "@/components/ui/progress";
import { cn } from "@/lib/utils";

function levelClass(value: number) {
  if (value >= 85) return "bg-metric-high";
  if (value >= 60) return "bg-metric-warn";
  return "bg-metric-ok";
}

interface MetricProps {
  label: string;
  value: number;
  detail?: string;
  className?: string;
}

export function Metric({ label, value, detail, className }: MetricProps) {
  const clamped = Math.min(Math.max(value, 0), 100);
  return (
    <div className={cn("space-y-1.5", className)}>
      <div className="flex items-baseline justify-between gap-2 text-sm">
        <span className="text-muted-foreground">{label}</span>
        <span className="tabular-nums">
          <span className="font-medium">{clamped.toFixed(1)}%</span>
          {detail && <span className="ml-2 text-xs text-muted-foreground">{detail}</span>}
        </span>
      </div>
      <Progress value={clamped} className="h-1.5 bg-muted" indicatorClassName={levelClass(clamped)} />
    </div>
  );
}
