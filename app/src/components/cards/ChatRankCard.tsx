import type { DashboardData } from "../../types";
import { EMPTY_STATE_MESSAGES } from "../../constants/messages";
import { CountUpNumber, dashboardIntroClassName, useDashboardViewportIntro } from "./dashboardAnimations";

export function ChatRankCard({ data }: { data: DashboardData["chatRank"] }) {
  const intro = useDashboardViewportIntro<HTMLElement>();
  return (
    <article ref={intro.ref} className={dashboardIntroClassName("panel ten-row-panel", intro)}>
      <header className="panel-header">
        <div>
          <strong>与我聊最多</strong>
          <span>仅单聊统计</span>
        </div>
      </header>
      <RankList
        rows={data.map((item) => ({ label: item.chat, count: item.count, sourceLabel: item.sourceLabel }))}
        animateCount={intro.isIntroAnimating}
        hasStarted={intro.hasStarted}
      />
    </article>
  );
}

export function RankList({
  rows,
  animateCount,
  hasStarted
}: {
  rows: Array<{ label: string; count: number; sourceLabel?: string }>;
  animateCount: boolean;
  hasStarted: boolean;
}) {
  const visibleRows = rows.slice(0, 10);
  const max = Math.max(...visibleRows.map((row) => row.count), 1);
  return (
    <div className="rank-list">
      {visibleRows.length === 0 ? (
        <div className="empty-state">{EMPTY_STATE_MESSAGES.noStats}</div>
      ) : (
        visibleRows.map((row, index) => (
          <div className="rank-row" key={`${row.label}-${row.sourceLabel ?? ""}`}>
            <b>{index + 1}</b>
            <span className="rank-name">
              <em>{row.label}</em>
              {row.sourceLabel && <small>{row.sourceLabel}</small>}
            </span>
            <div className="rank-bar" aria-hidden="true">
              <i className="rank-bar-fill" style={{ width: `${(row.count / max) * 100}%` }} />
            </div>
            <strong className="rank-count">
              <CountUpNumber value={row.count} animate={animateCount} hasStarted={hasStarted} />
            </strong>
          </div>
        ))
      )}
    </div>
  );
}
