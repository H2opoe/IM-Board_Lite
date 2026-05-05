import { EMPTY_STATE_MESSAGES } from "../../constants/messages";
import type { DashboardData } from "../../types";
import { sourceChatsTitle } from "../../utils/sources";
import { Settings } from "lucide-react";
import { CountUpNumber, dashboardIntroClassName, useDashboardViewportIntro } from "./dashboardAnimations";

function topicSummary(topic: DashboardData["topics"][number]) {
  let summary = topic.summary.trim();
  const chatNames = [
    ...(topic.sourceChats?.map((source) => source.chatName) ?? []),
    ...(topic.sources?.flatMap((source) => source.chats) ?? [])
  ]
    .map((name) => name.trim())
    .filter(Boolean)
    .sort((a, b) => b.length - a.length);

  for (const chatName of chatNames) {
    const escaped = chatName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    summary = summary.replace(new RegExp(`^${escaped}(?:中|里|内)?(?:讨论|提到|涉及)?`, "u"), "");
  }

  return summary.replace(/^中/, "").replace(/^[，,、：:\s]+/, "") || topic.summary;
}

export function TopicCard({
  topics,
  aiStatus,
  onConfigureAi
}: {
  topics: DashboardData["topics"];
  aiStatus: DashboardData["aiStatus"];
  onConfigureAi: () => void;
}) {
  const intro = useDashboardViewportIntro<HTMLElement>();
  const visibleTopics = topics
    .filter((topic) => (topic.count ?? 0) > 3)
    .sort((a, b) => (b.count ?? 0) - (a.count ?? 0))
    .slice(0, 6);

  return (
    <article ref={intro.ref} className={dashboardIntroClassName("panel ten-row-panel topic-panel", intro)}>
      <header className="panel-header">
        <div>
          <strong>热门讨论话题</strong>
          <span>
            {aiStatus === "not_configured" ? (
              "配置AI后显示"
            ) : (
              <>
                <b className="dashboard-count">
                  <CountUpNumber value={visibleTopics.length} animate={intro.isIntroAnimating} hasStarted={intro.hasStarted} />
                </b>
                个话题
              </>
            )}
          </span>
        </div>
      </header>
      <div className="topic-list">
        {aiStatus === "not_configured" ? (
          <div className="empty-state">
            <div className="empty-state-content">
              <span>配置AI后即可生成讨论话题</span>
              <button className="primary-button compact-button" onClick={onConfigureAi}>
                <Settings size={15} />
                AI配置
              </button>
            </div>
          </div>
        ) : aiStatus === "analyzing" ? (
          <div className="empty-state">{EMPTY_STATE_MESSAGES.aiTopicPending}</div>
        ) : visibleTopics.length === 0 ? (
          <div className="empty-state">{EMPTY_STATE_MESSAGES.noActionItems}</div>
        ) : (
          visibleTopics.map((topic, index) => (
            <div className="topic-row" key={topic.title} title={sourceChatsTitle(topic.sources)}>
              <span className="rank-index">{index + 1}</span>
              <div>
                <strong>{topic.title}</strong>
                <p>{topicSummary(topic)}</p>
              </div>
              {topic.count && (
                <span className="topic-count">
                  <CountUpNumber value={topic.count} animate={intro.isIntroAnimating} hasStarted={intro.hasStarted} />
                </span>
              )}
            </div>
          ))
        )}
      </div>
    </article>
  );
}
