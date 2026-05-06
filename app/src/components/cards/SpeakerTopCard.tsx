import { RankList } from "./ChatRankCard";
import type { DashboardData } from "../../features/dashboard/model/types";
import { dashboardIntroClassName, useDashboardViewportIntro } from "./dashboardAnimations";

export function SpeakerTopCard({ data }: { data: DashboardData["speakerTop"] }) {
  const intro = useDashboardViewportIntro<HTMLElement>();
  return (
    <article ref={intro.ref} className={dashboardIntroClassName("panel ten-row-panel", intro)}>
      <header className="panel-header">
        <div>
          <strong>发言Top10</strong>
          <span>按发送人统计</span>
        </div>
      </header>
      <RankList
        rows={data.map((item) => ({ label: item.speaker, count: item.count, sourceLabel: item.sourceLabel }))}
        animateCount={intro.isIntroAnimating}
        hasStarted={intro.hasStarted}
      />
    </article>
  );
}
