import { Bar, BarChart, CartesianGrid, LabelList, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { DashboardData } from "../../features/dashboard/model/types";
import { sourceStatsTitle } from "../../utils/sources";
import { chartIntroAnimationDuration, dashboardIntroClassName, useAnimatedNumber, useDashboardViewportIntro } from "./dashboardAnimations";

export function ActivityChartCard({ data }: { data: DashboardData["hourlyActivity"] }) {
  const intro = useDashboardViewportIntro<HTMLElement>();
  const visibleData = intro.hasStarted ? data : data.map((entry) => ({ ...entry, count: 0 }));

  return (
    <article ref={intro.ref} className={dashboardIntroClassName("panel analytics-panel", intro)}>
      <header className="panel-header">
        <div>
          <strong>24小时活跃分布</strong>
          <span>每小时消息数</span>
        </div>
      </header>
      <div className="chart-box">
        <ResponsiveContainer width="100%" height="100%">
            <BarChart data={visibleData} margin={{ top: 18, right: 4, left: 0, bottom: 0 }}>
            <CartesianGrid stroke="rgba(255,255,255,0.06)" strokeDasharray="3 3" vertical={false} />
            <XAxis dataKey="hour" interval={2} tickLine={false} axisLine={false} tick={{ fill: "var(--im-text-tertiary)", fontSize: 11 }} />
            <YAxis
              tickLine={false}
              axisLine={false}
              width={36}
              allowDecimals={false}
              domain={[0, "dataMax + 3"]}
              tick={{ fill: "var(--im-text-tertiary)", fontSize: 11 }}
            />
            <Tooltip content={<ActivityTooltip />} />
            <Bar
              key={intro.hasStarted ? "visible" : "pending"}
              dataKey="count"
              fill="var(--im-chart-green)"
              radius={[5, 5, 0, 0]}
              isAnimationActive={intro.isIntroAnimating}
              animationBegin={120}
              animationDuration={chartIntroAnimationDuration}
              animationEasing="ease-out"
            >
              <LabelList
                dataKey="count"
                content={<ActivityBarValueLabel isIntroAnimating={intro.isIntroAnimating} hasStarted={intro.hasStarted} />}
              />
            </Bar>
          </BarChart>
        </ResponsiveContainer>
      </div>
    </article>
  );
}

function ActivityBarValueLabel({
  x,
  y,
  width,
  value,
  isIntroAnimating,
  hasStarted
}: {
  x?: number | string;
  y?: number | string;
  width?: number | string;
  value?: number | string;
  isIntroAnimating?: boolean;
  hasStarted?: boolean;
}) {
  const count = Number(value ?? 0);
  const labelX = Number(x ?? 0) + Number(width ?? 0) / 2;
  const labelY = Math.max(12, Number(y ?? 0) - 6);
  const animatedCount = useAnimatedNumber(count, Boolean(isIntroAnimating), Boolean(hasStarted), chartIntroAnimationDuration);

  if (!count || !Number.isFinite(labelX) || !Number.isFinite(labelY)) return null;
  return (
    <text x={labelX} y={labelY} textAnchor="middle" className="activity-bar-value">
      {animatedCount}
    </text>
  );
}

function ActivityTooltip({ active, payload, label }: { active?: boolean; payload?: Array<{ payload: DashboardData["hourlyActivity"][number] }>; label?: string }) {
  if (!active || !payload?.length) return null;
  const row = payload[0].payload;
  return (
    <div className="chart-tooltip">
      <strong>{label}:00</strong>
      <span>总计{row.count}</span>
      <pre>{sourceStatsTitle(row.sources)}</pre>
    </div>
  );
}
