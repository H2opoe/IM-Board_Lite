import { useEffect, useRef, useState } from "react";
import { Clock3, RotateCcw, RefreshCw, Sparkles, StopCircle } from "lucide-react";
import { getDashboard, markActionItem } from "../../features/dashboard/api/dashboardApi";
import type { SyncProgressNotice, SyncUiState } from "../../features/sync/hooks/useSyncController";
import { ActivityChartCard } from "../../components/cards/ActivityChartCard";
import { ActionListCard } from "../../components/cards/ActionListCard";
import { ChatRankCard } from "../../components/cards/ChatRankCard";
import { MessageTypePieCard } from "../../components/cards/MessageTypePieCard";
import { MetricCard } from "../../components/cards/MetricCard";
import { SpeakerTopCard } from "../../components/cards/SpeakerTopCard";
import { TopicCard } from "../../components/cards/TopicCard";
import { WordCloudCard } from "../../components/cards/WordCloudCard";
import { FloatingNotice, FloatingNoticeStack, type FloatingNoticeVariant } from "../../components/shared/FloatingNotice";
import { PagedTextBlock } from "../../components/shared/PagedTextBlock";
import { DASHBOARD_MESSAGES } from "../../constants/messages";
import type { ActionItem, DashboardData } from "../../features/dashboard/model/types";
import type { ImProfile } from "../../features/profiles/model/types";
import { profileDisplayName } from "../../utils/profiles";
import { isSyncMessageError } from "../../features/sync/messages/syncMessages";

interface Props {
  data: DashboardData;
  profiles: ImProfile[];
  activeProfileId: string;
  syncState: SyncUiState;
  syncMessage: string;
  syncMessagePages: string[];
  syncProgressNotices: SyncProgressNotice[];
  syncFrequencyMinutes: number;
  isSyncCancelArmed: boolean;
  isDemoMode: boolean;
  onOpenSource: (action: ActionItem) => void;
  onDashboardChange: (data: DashboardData) => void;
  onSyncNow: () => void;
  onFullResync: () => void;
  onRetryAiAnalysis: () => void;
  onSyncFrequencyChange: (minutes: number) => void;
  onDismissSyncMessage: () => void;
  onDismissSyncProgressNotice: (noticeId: string) => void;
  onConfigureAi: () => void;
}

export function DashboardPage({
  data,
  profiles,
  activeProfileId,
  syncState,
  syncMessage,
  syncMessagePages,
  syncProgressNotices,
  syncFrequencyMinutes,
  isSyncCancelArmed,
  isDemoMode,
  onOpenSource,
  onDashboardChange,
  onSyncNow,
  onFullResync,
  onRetryAiAnalysis,
  onSyncFrequencyChange,
  onDismissSyncMessage,
  onDismissSyncProgressNotice,
  onConfigureAi
}: Props) {
  const [isFrequencyOpen, setIsFrequencyOpen] = useState(false);
  const [frequencyDraft, setFrequencyDraft] = useState(String(syncFrequencyMinutes));
  const frequencyControlRef = useRef<HTMLDivElement | null>(null);
  const activeProfile = profiles.find((profile) => profile.id === activeProfileId);
  const activeName = activeProfileId === "aggregate" ? "聚合看板" : activeProfile ? profileDisplayName(activeProfile) : "单Profile看板";
  const visibleSyncMessage = syncMessagePages.length > 0 ? syncMessagePages.join("\n\n") : syncMessage;
  const hasSyncErrorMessage = [syncMessage, ...syncMessagePages].some((message) => message && isSyncMessageError(message));
  const syncNoticeVariant: FloatingNoticeVariant = syncState === "failed" || hasSyncErrorMessage ? "error" : syncState === "done" && syncMessagePages.length === 0 ? "success" : "info";
  const canCancelCurrentRun = syncState === "syncing" || syncState === "analyzing";
  const syncStatusLabels: Record<string, string> = DASHBOARD_MESSAGES.statusLabels.sync;
  const aiStatusLabels: Record<string, string> = DASHBOARD_MESSAGES.statusLabels.ai;
  const syncStatusLabel = syncStatusLabels[data.syncStatus] ?? data.syncStatus;
  const aiStatusLabel = aiStatusLabels[data.aiStatus] ?? data.aiStatus;

  async function complete(item: ActionItem) {
    await markActionItem(item.id, "done");
    const next = await getDashboard(activeProfileId);
    onDashboardChange(next);
  }

  async function reopen(item: ActionItem) {
    await markActionItem(item.id, "open");
    const next = await getDashboard(activeProfileId);
    onDashboardChange(next);
  }

  useEffect(() => {
    if (!isFrequencyOpen) return undefined;
    function closeOnOutsidePointer(event: PointerEvent) {
      if (frequencyControlRef.current?.contains(event.target as Node)) return;
      setIsFrequencyOpen(false);
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setIsFrequencyOpen(false);
    }
    document.addEventListener("pointerdown", closeOnOutsidePointer);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePointer);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [isFrequencyOpen]);

  function saveFrequency() {
    const next = Math.max(1, Number.parseInt(frequencyDraft, 10) || syncFrequencyMinutes);
    onSyncFrequencyChange(next);
    setFrequencyDraft(String(next));
    setIsFrequencyOpen(false);
  }

  return (
    <div className="dashboard-page">
      <header className="topbar">
        <div>
          <div className="topbar-title-row">
            <h1>{activeName}</h1>
            {isDemoMode && <span className="demo-mode-badge">演示数据</span>}
          </div>
          <span>
            {data.day}｜同步状态 {syncStatusLabel}｜AI状态 {aiStatusLabel}
          </span>
        </div>
        <div className="topbar-actions">
          <div className="frequency-control" ref={frequencyControlRef}>
            <button
              className="secondary-button"
              onClick={() => {
                setFrequencyDraft(String(syncFrequencyMinutes));
                setIsFrequencyOpen((open) => !open);
              }}
            >
              <Clock3 size={17} />
              {syncFrequencyMinutes}分钟/次
            </button>
            {isFrequencyOpen && (
              <div className="frequency-popover">
                <label>
                  <span>同步频率</span>
                  <input
                    type="number"
                    min={1}
                    value={frequencyDraft}
                    onChange={(event) => setFrequencyDraft(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") saveFrequency();
                    }}
                  />
                </label>
                <span>分钟/次</span>
                <button className="primary-button compact-button" onClick={saveFrequency}>
                  保存
                </button>
                <div className="frequency-resync">
                  <div className="frequency-maintenance-row">
                    <p>{DASHBOARD_MESSAGES.retryAnalysisDescription}</p>
                    <button
                      className="secondary-button compact-button"
                      onClick={() => {
                        setIsFrequencyOpen(false);
                        onRetryAiAnalysis();
                      }}
                    >
                      <Sparkles size={15} />
                      重新分析
                    </button>
                  </div>
                  <div className="frequency-maintenance-row">
                    <p>{DASHBOARD_MESSAGES.fullResyncDescription}</p>
                    <button
                      className="secondary-button compact-button"
                      onClick={() => {
                        setIsFrequencyOpen(false);
                        onFullResync();
                      }}
                    >
                      <RotateCcw size={15} />
                      重新同步
                    </button>
                  </div>
                </div>
              </div>
            )}
          </div>
          <button
            className={canCancelCurrentRun && isSyncCancelArmed ? "danger-button sync-action-button" : "primary-button sync-action-button"}
            onClick={onSyncNow}
          >
            {canCancelCurrentRun && isSyncCancelArmed ? (
              <StopCircle size={18} />
            ) : (
              <RefreshCw size={18} className={syncState === "syncing" || syncState === "analyzing" ? "spin" : undefined} />
            )}
            {syncState === "syncing"
              ? isSyncCancelArmed
                ? "终止同步"
                : "同步中"
              : syncState === "analyzing"
                ? isSyncCancelArmed
                  ? "终止分析"
                  : "分析中"
                : "立即同步"}
          </button>
        </div>
      </header>

      {syncProgressNotices.length > 0 && (
        <FloatingNoticeStack>
          {[...syncProgressNotices].reverse().map((notice) => (
            <FloatingNotice
              key={notice.id}
              message={notice.message}
              variant={notice.variant}
              withinLayer
              autoCloseMs={notice.variant === "success" ? undefined : false}
              onClose={() => onDismissSyncProgressNotice(notice.id)}
            />
          ))}
        </FloatingNoticeStack>
      )}

      {syncProgressNotices.length === 0 && syncMessage && (
        <FloatingNoticeStack>
          <FloatingNotice
            variant={syncNoticeVariant}
            withinLayer
            autoCloseMs={syncNoticeVariant === "success" ? undefined : false}
            onClose={onDismissSyncMessage}
          >
            <PagedTextBlock
              text={visibleSyncMessage}
              className="sync-message-content"
              controlsClassName="sync-message-pager"
              compactCopy
            />
          </FloatingNotice>
        </FloatingNoticeStack>
      )}

      <section className="metric-grid">
        {data.metrics.map((metric) => (
          <MetricCard key={metric.key} metric={metric} />
        ))}
      </section>

      <section className="dashboard-grid">
        <ActionListCard
          title="待我回复"
          aiStatus={data.aiStatus}
          items={data.replies}
          onComplete={complete}
          onReopen={reopen}
          onOpenSource={onOpenSource}
          onConfigureAi={onConfigureAi}
        />
        <ActionListCard
          title="待办事项"
          aiStatus={data.aiStatus}
          items={data.tasks}
          onComplete={complete}
          onReopen={reopen}
          onOpenSource={onOpenSource}
          onConfigureAi={onConfigureAi}
        />
        <TopicCard topics={data.topics} aiStatus={data.aiStatus} onConfigureAi={onConfigureAi} />
        <WordCloudCard keywords={data.keywords} />
        <ActivityChartCard data={data.hourlyActivity} />
        <MessageTypePieCard data={data.messageTypes} />
        <ChatRankCard data={data.chatRank} />
        <SpeakerTopCard data={data.speakerTop} />
      </section>
    </div>
  );
}
