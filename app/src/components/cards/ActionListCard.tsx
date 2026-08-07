import { Check, ExternalLink, RotateCcw, Settings } from "lucide-react";
import { useLayoutEffect, useMemo, useRef } from "react";
import { EMPTY_STATE_MESSAGES } from "../../constants/messages";
import type { ActionItem, DashboardData } from "../../features/dashboard/model/types";
import { formatRelativeDateTime } from "../../utils/dates";
import { CountUpNumber, dashboardIntroClassName, useDashboardViewportIntro } from "./dashboardAnimations";

interface Props {
  title: string;
  aiStatus: DashboardData["aiStatus"];
  items: ActionItem[];
  onComplete: (item: ActionItem) => void;
  onReopen: (item: ActionItem) => void;
  onOpenSource: (action: ActionItem) => void;
  onConfigureAi: () => void;
}

const priorityLabel = {
  high: "高",
  medium: "中",
  low: "低"
};

function actionMeta(item: ActionItem) {
  return `${item.chatName}·${item.sourceLabel}·${formatRelativeDateTime(item.sourceMessageAt)}`;
}

function actionStatusOrder(status: ActionItem["status"]) {
  if (status === "open") return 0;
  if (status === "done") return 1;
  return 2;
}

function actionSourceTime(item: ActionItem) {
  const timestamp = new Date(item.sourceMessageAt).getTime();
  return Number.isNaN(timestamp) ? 0 : timestamp;
}

function actionCompletedTime(item: ActionItem) {
  const timestamp = new Date(item.completedAt || item.lastUpdatedAt).getTime();
  return Number.isNaN(timestamp) ? 0 : timestamp;
}

function compareActions(left: ActionItem, right: ActionItem) {
  const statusDelta = actionStatusOrder(left.status) - actionStatusOrder(right.status);
  if (statusDelta !== 0) return statusDelta;

  if (left.status === "open") return actionSourceTime(left) - actionSourceTime(right);
  if (left.status === "done") return actionCompletedTime(right) - actionCompletedTime(left);
  return actionSourceTime(right) - actionSourceTime(left);
}

function canPlayActionReorderAnimation() {
  return typeof window !== "undefined" && !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function ActionListCard({ title, aiStatus, items, onComplete, onReopen, onOpenSource, onConfigureAi }: Props) {
  const intro = useDashboardViewportIntro<HTMLElement>();
  const itemRefs = useRef(new Map<string, HTMLDivElement>());
  const itemRects = useRef(new Map<string, DOMRect>());
  const itemAnimations = useRef(new Map<string, Animation>());
  const openCount = items.filter((item) => item.status === "open").length;
  const sortedItems = useMemo(() => [...items].sort(compareActions), [items]);
  const sortedItemKey = sortedItems.map((item) => `${item.id}:${item.status}:${item.sourceMessageAt}`).join("|");
  let openSequence = 0;

  useLayoutEffect(() => {
    const nextRects = new Map<string, DOMRect>();

    for (const item of sortedItems) {
      const element = itemRefs.current.get(item.id);
      if (element) nextRects.set(item.id, element.getBoundingClientRect());
    }

    if (canPlayActionReorderAnimation() && itemRects.current.size > 0) {
      for (const item of sortedItems) {
        const element = itemRefs.current.get(item.id);
        const previousRect = itemRects.current.get(item.id);
        const nextRect = nextRects.get(item.id);
        if (!element || !previousRect || !nextRect) continue;

        const deltaX = previousRect.left - nextRect.left;
        const deltaY = previousRect.top - nextRect.top;
        if (Math.abs(deltaX) < 0.5 && Math.abs(deltaY) < 0.5) continue;

        itemAnimations.current.get(item.id)?.cancel();
        element.classList.add("action-item-reordering");
        const animation = element.animate(
          [
            { transform: `translate(${deltaX}px, ${deltaY}px)` },
            { transform: "translate(0, 0)" }
          ],
          {
            duration: 320,
            easing: "cubic-bezier(0.16, 1, 0.3, 1)"
          }
        );
        itemAnimations.current.set(item.id, animation);
        animation.onfinish = () => {
          itemAnimations.current.delete(item.id);
          element.classList.remove("action-item-reordering");
        };
        animation.oncancel = () => {
          itemAnimations.current.delete(item.id);
          element.classList.remove("action-item-reordering");
        };
      }
    }

    itemRects.current = nextRects;
  }, [sortedItemKey, sortedItems]);

  return (
    <article ref={intro.ref} className={dashboardIntroClassName("panel fixed-list", intro)}>
      <header className="panel-header">
        <div>
          <strong>{title}</strong>
          <span>
            {aiStatus === "not_configured" ? (
              "配置AI后显示"
            ) : (
              <>
                <b className="dashboard-count">
                  <CountUpNumber value={openCount} animate={intro.isIntroAnimating} hasStarted={intro.hasStarted} />
                </b>
                个未完成
              </>
            )}
          </span>
        </div>
      </header>

      {aiStatus === "not_configured" ? (
        <div className="empty-state">
          <div className="empty-state-content">
            <span>配置AI后即可识别和操作</span>
            <button className="primary-button compact-button" onClick={onConfigureAi}>
              <Settings size={15} />
              AI配置
            </button>
          </div>
        </div>
      ) : aiStatus === "analyzing" && items.length === 0 ? (
        <div className="empty-state">{EMPTY_STATE_MESSAGES.aiActionPending}</div>
      ) : items.length === 0 ? (
        <div className="empty-state">{EMPTY_STATE_MESSAGES.noActionItems}</div>
      ) : (
        <div className="action-list">
          {sortedItems.map((item) => {
            const sequence = item.status === "open" ? (openSequence += 1) : null;
            return (
              <div
                key={item.id}
                ref={(element) => {
                  if (element) {
                    itemRefs.current.set(item.id, element);
                  } else {
                    itemRefs.current.delete(item.id);
                  }
                }}
                className={item.status === "done" ? "action-item done" : "action-item"}
              >
                <div className="action-main">
                  {sequence === null ? (
                    <span className="action-index done-check" title="已完成">
                      <Check size={13} />
                    </span>
                  ) : (
                    <span className="action-index open-index" title={`优先级：${priorityLabel[item.priority]}`}>
                      {sequence}
                    </span>
                  )}
                  <div>
                    <strong>{item.title}</strong>
                    <p>{item.description}</p>
                    <small title={actionMeta(item)}>
                      <span className="action-meta-text">{actionMeta(item)}</span>
                    </small>
                  </div>
                </div>
                <div className="action-tools">
                  <button className="icon-button" onClick={() => onOpenSource(item)} aria-label="查看来源">
                    <ExternalLink size={16} />
                  </button>
                  {item.status === "open" && (
                    <button className="icon-button" onClick={() => onComplete(item)} aria-label="完成">
                      <Check size={17} />
                    </button>
                  )}
                  {item.status !== "open" && (
                    <button className="icon-button" onClick={() => onReopen(item)} aria-label="标为未完成" title="标为未完成">
                      <RotateCcw size={16} />
                    </button>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </article>
  );
}
