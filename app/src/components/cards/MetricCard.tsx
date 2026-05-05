import { MessageSquare, Send, SquareCheckBig, Users } from "lucide-react";
import type { DashboardMetric } from "../../types";
import { sourceStatsTitle } from "../../utils/sources";
import { CountUpNumber, dashboardIntroClassName, useDashboardViewportIntro } from "./dashboardAnimations";

const icons = {
  messages: MessageSquare,
  replies: Send,
  tasks: SquareCheckBig,
  chats: Users
};

export function MetricCard({ metric }: { metric: DashboardMetric }) {
  const intro = useDashboardViewportIntro<HTMLElement>();
  const Icon = icons[metric.key as keyof typeof icons] ?? MessageSquare;
  return (
    <article ref={intro.ref} className={dashboardIntroClassName("metric-card", intro)} title={sourceStatsTitle(metric.sources)}>
      <div>
        <span>{metric.label}</span>
        <strong className="metric-value">
          <CountUpNumber value={metric.value} animate={intro.isIntroAnimating} hasStarted={intro.hasStarted} />
        </strong>
      </div>
      <Icon size={22} />
    </article>
  );
}
