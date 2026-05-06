import { Cell, Pie, PieChart, ResponsiveContainer, Tooltip } from "recharts";
import { EMPTY_STATE_MESSAGES } from "../../constants/messages";
import type { DashboardData } from "../../features/dashboard/model/types";
import { sourceStatsTitle } from "../../utils/sources";
import { CountUpNumber, chartIntroAnimationDuration, dashboardIntroClassName, useDashboardViewportIntro } from "./dashboardAnimations";

const COLORS = [
  "var(--im-chart-green)",
  "var(--im-chart-orange)",
  "var(--im-chart-blue)",
  "var(--im-chart-yellow)",
  "var(--im-chart-purple)",
  "var(--im-chart-gray)"
];
const TYPE_LABELS: Record<string, string> = {
  text: "文本",
  image: "图片",
  file: "文件",
  link: "链接",
  voice: "语音",
  video: "视频",
  system: "系统",
  emoji: "表情",
  location: "位置",
  calendar: "日程",
  todo: "待办",
  unknown: "未知"
};

export function MessageTypePieCard({ data }: { data: DashboardData["messageTypes"] }) {
  const intro = useDashboardViewportIntro<HTMLElement>();
  const displayData = data.map((entry) => ({
    ...entry,
    count: intro.hasStarted ? entry.count : 0,
    label: TYPE_LABELS[entry.type] ?? entry.type
  }));

  return (
    <article ref={intro.ref} className={dashboardIntroClassName("panel analytics-panel", intro)}>
      <header className="panel-header">
        <div>
          <strong>消息类型分布</strong>
          <span>各类消息占比</span>
        </div>
      </header>
      <div className="pie-layout">
        {displayData.length === 0 ? (
          <div className="empty-state">{EMPTY_STATE_MESSAGES.noMessageTypes}</div>
        ) : (
          <>
            <div className="pie-box">
              <ResponsiveContainer width="100%" height="100%">
                <PieChart>
                  <Pie
                    key={intro.hasStarted ? "visible" : "pending"}
                    data={displayData}
                    dataKey="count"
                    nameKey="label"
                    innerRadius="45%"
                    outerRadius="78%"
                    paddingAngle={2}
                    isAnimationActive={intro.isIntroAnimating}
                    animationBegin={140}
                    animationDuration={chartIntroAnimationDuration}
                    animationEasing="ease-out"
                  >
                    {displayData.map((entry, index) => (
                      <Cell key={entry.type} fill={COLORS[index % COLORS.length]} />
                    ))}
                  </Pie>
                  <Tooltip content={<MessageTypeTooltip />} />
                </PieChart>
              </ResponsiveContainer>
            </div>
            <div className="legend-list">
              {displayData.map((entry, index) => (
                <span key={entry.type}>
                  <i style={{ background: COLORS[index % COLORS.length] }} />
                  {entry.label}{" "}
                  <strong className="legend-count">
                    <CountUpNumber value={entry.count} animate={intro.isIntroAnimating} hasStarted={intro.hasStarted} />
                  </strong>
                </span>
              ))}
            </div>
          </>
        )}
      </div>
    </article>
  );
}

function MessageTypeTooltip({ active, payload }: { active?: boolean; payload?: Array<{ payload: DashboardData["messageTypes"][number] & { label: string } }> }) {
  if (!active || !payload?.length) return null;
  const row = payload[0].payload;
  return (
    <div className="chart-tooltip">
      <strong>{row.label}</strong>
      <span>总计{row.count}</span>
      <pre>{sourceStatsTitle(row.sources)}</pre>
    </div>
  );
}
