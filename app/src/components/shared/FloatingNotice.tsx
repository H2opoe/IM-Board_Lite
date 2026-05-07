import {
  type CSSProperties,
  Children,
  cloneElement,
  type PointerEvent,
  type ReactNode,
  type WheelEvent,
  isValidElement,
  useEffect,
  useRef,
  useState
} from "react";
import { AlertCircle, CheckCircle2, Info, X } from "lucide-react";
import { PagedTextBlock } from "./PagedTextBlock";

export type FloatingNoticeScope = "page" | "modal";
export type FloatingNoticeVariant = "info" | "success" | "error";

interface Props {
  message?: string;
  children?: ReactNode;
  variant?: FloatingNoticeVariant;
  scope?: FloatingNoticeScope;
  withinLayer?: boolean;
  autoCloseMs?: false;
  autoCloseDelayMs?: number;
  onClose: () => void;
}

const NOTICE_AUTO_CLOSE_MS = 5_000;
const STACK_NOTICE_ENTER_DELAY_MS = 500;
const STACK_TURN_DISTANCE_PX = 26;
const NOTICE_CLOSE_ANIMATION_MS = 240;

interface FloatingNoticeStackProps {
  children: ReactNode;
  scope?: FloatingNoticeScope;
}

type StackStyle = CSSProperties & {
  "--notice-count"?: number;
  "--notice-focus"?: number;
  "--notice-index"?: number;
  "--notice-display-index"?: number;
  "--notice-enter-delay"?: string;
  "--notice-stack-height"?: string;
};

function resolveNoticeDisplayIndex(baseDisplayIndex: number, visibleCount: number, turnProgress: number) {
  if (visibleCount <= 1 || turnProgress === 0) return baseDisplayIndex;

  if (turnProgress > 0) {
    if (baseDisplayIndex === 0) {
      return turnProgress * (visibleCount - 1);
    }
    return baseDisplayIndex - turnProgress;
  }

  const backwardProgress = Math.abs(turnProgress);
  if (baseDisplayIndex === visibleCount - 1) {
    return (visibleCount - 1) * (1 - backwardProgress);
  }
  return baseDisplayIndex + backwardProgress;
}

export function FloatingNoticeStack({ children, scope = "page" }: FloatingNoticeStackProps) {
  const notices = Children.toArray(children).filter(Boolean);
  const noticeKeys = notices.map((notice, index) => String(isValidElement(notice) && notice.key !== null ? notice.key : index));
  const noticeOrderKey = noticeKeys.join("|");
  const [focusIndex, setFocusIndex] = useState(0);
  const [turnProgress, setTurnProgress] = useState(0);
  const [revealedCount, setRevealedCount] = useState(0);
  const previousNoticeKeysRef = useRef<string[]>([]);
  const lastPointerYRef = useRef<number | null>(null);
  const pointerRemainderRef = useRef(0);

  useEffect(() => {
    const previousNoticeKeys = previousNoticeKeysRef.current;
    const previousNoticeKeySet = new Set(previousNoticeKeys);
    const hasSameNoticeSet =
      previousNoticeKeys.length === noticeKeys.length && noticeKeys.every((noticeKey) => previousNoticeKeySet.has(noticeKey));
    const addedNoticeCount = noticeKeys.filter((noticeKey) => !previousNoticeKeySet.has(noticeKey)).length;

    previousNoticeKeysRef.current = noticeKeys;
    lastPointerYRef.current = null;
    pointerRemainderRef.current = 0;
    setTurnProgress(0);

    if (notices.length === 0) {
      setFocusIndex(0);
      setRevealedCount(0);
      return undefined;
    }

    setFocusIndex((currentIndex) => Math.min(currentIndex, notices.length - 1));

    if (hasSameNoticeSet) {
      setRevealedCount(notices.length);
      return undefined;
    }

    if (previousNoticeKeys.length > 0) {
      setRevealedCount(Math.max(0, notices.length - addedNoticeCount));
      const timers = Array.from({ length: addedNoticeCount }, (_, index) =>
        window.setTimeout(() => {
          setRevealedCount(notices.length - addedNoticeCount + index + 1);
        }, index * STACK_NOTICE_ENTER_DELAY_MS)
      );
      return () => {
        timers.forEach((timer) => window.clearTimeout(timer));
      };
    }

    setFocusIndex(0);
    setRevealedCount(0);
    const timers = notices.map((_, index) =>
      window.setTimeout(() => {
        setRevealedCount(index + 1);
      }, index * STACK_NOTICE_ENTER_DELAY_MS)
    );
    return () => {
      timers.forEach((timer) => window.clearTimeout(timer));
    };
  }, [noticeOrderKey]);

  if (notices.length === 0) return null;

  const visibleNotices = notices
    .map((notice, index) => ({ index, notice }))
    .slice(Math.max(0, notices.length - revealedCount));
  const visibleCount = visibleNotices.length;

  function handlePointerEnter(event: PointerEvent<HTMLDivElement>) {
    lastPointerYRef.current = event.clientY;
    pointerRemainderRef.current = 0;
  }

  function handlePointerLeave() {
    lastPointerYRef.current = null;
    pointerRemainderRef.current = 0;
    setTurnProgress(0);
  }

  function applyTurnDelta(deltaY: number) {
    if (visibleCount <= 1) return;
    let nextRemainder = pointerRemainderRef.current + deltaY;

    while (Math.abs(nextRemainder) >= STACK_TURN_DISTANCE_PX) {
      const direction = nextRemainder > 0 ? 1 : -1;
      setFocusIndex((currentIndex) => (currentIndex + direction + visibleCount) % visibleCount);
      nextRemainder -= direction * STACK_TURN_DISTANCE_PX;
    }

    pointerRemainderRef.current = nextRemainder;
    setTurnProgress(nextRemainder / STACK_TURN_DISTANCE_PX);
  }

  function handlePointerMove(event: PointerEvent<HTMLDivElement>) {
    if (visibleCount <= 1) return;
    const lastPointerY = lastPointerYRef.current ?? event.clientY;
    const deltaY = event.clientY - lastPointerY;
    lastPointerYRef.current = event.clientY;

    applyTurnDelta(deltaY);
  }

  function handleWheel(event: WheelEvent<HTMLDivElement>) {
    if (visibleCount <= 1) return;
    event.preventDefault();
    applyTurnDelta(event.deltaY);
  }

  const layerStyle: StackStyle = {
    "--notice-count": visibleCount,
    "--notice-focus": focusIndex,
    "--notice-stack-height": `${54 + Math.max(0, visibleCount - 1) * 27}px`
  };

  return (
    <div
      className={`floating-notice-layer ${scope} stacked carousel`}
      style={layerStyle}
      onPointerEnter={handlePointerEnter}
      onPointerLeave={handlePointerLeave}
      onPointerMove={handlePointerMove}
      onWheel={handleWheel}
    >
      <div className="floating-notice-stack">
        {visibleNotices.map(({ index: noticeIndex, notice }, visibleIndex) => {
          const baseDisplayIndex = (visibleIndex - focusIndex + visibleCount) % visibleCount;
          const displayIndex = resolveNoticeDisplayIndex(baseDisplayIndex, visibleCount, turnProgress);

          return (
            <div
              key={isValidElement(notice) && notice.key !== null ? notice.key : noticeIndex}
              className="floating-notice-stack-item"
              style={
                {
                  "--notice-index": visibleIndex,
                  "--notice-display-index": displayIndex,
                  zIndex: Math.max(1, Math.round((visibleCount - displayIndex) * 10))
                } as StackStyle
              }
            >
              {isValidElement<Props>(notice) ? cloneElement(notice, { autoCloseDelayMs: (notices.length - 1 - noticeIndex) * STACK_NOTICE_ENTER_DELAY_MS }) : notice}
            </div>
          );
        })}
      </div>
    </div>
  );
}

export function FloatingNotice({
  message,
  children,
  variant = "info",
  scope = "page",
  withinLayer = false,
  autoCloseMs,
  autoCloseDelayMs = 0,
  onClose
}: Props) {
  const onCloseRef = useRef(onClose);
  const [isSingleLineMessage, setIsSingleLineMessage] = useState(false);
  const [isClosing, setIsClosing] = useState(false);
  const hasContent = Boolean(message || children);
  const effectiveAutoCloseMs = autoCloseMs === false ? undefined : variant === "success" ? NOTICE_AUTO_CLOSE_MS : undefined;

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    setIsClosing(false);
  }, [children, message, variant]);

  function requestClose() {
    if (isClosing) return;
    setIsClosing(true);
    window.setTimeout(() => {
      onCloseRef.current();
    }, NOTICE_CLOSE_ANIMATION_MS);
  }

  useEffect(() => {
    if (!hasContent || !effectiveAutoCloseMs || isClosing) return undefined;
    const timer = window.setTimeout(() => {
      requestClose();
    }, autoCloseDelayMs + effectiveAutoCloseMs);
    return () => window.clearTimeout(timer);
  }, [autoCloseDelayMs, effectiveAutoCloseMs, hasContent, isClosing, message, variant]);

  if (!hasContent) return null;

  const Icon = variant === "success" ? CheckCircle2 : variant === "error" ? AlertCircle : Info;

  const notice = (
    <div
      className={`floating-notice ${variant} ${isSingleLineMessage ? "single-line-message" : ""}`.trim()}
      data-closing={isClosing ? "true" : undefined}
      role={variant === "error" ? "alert" : "status"}
      aria-live={variant === "error" ? "assertive" : "polite"}
    >
      <Icon className="floating-notice-icon" size={17} />
      <div className="floating-notice-body">
        {children ?? (
          <PagedTextBlock
            text={message ?? ""}
            textClassName="floating-notice-message"
            controlsClassName="floating-notice-message-pager"
            element="span"
            compactCopy
            onSingleLineChange={setIsSingleLineMessage}
          />
        )}
      </div>
      <button className="floating-notice-close" onClick={requestClose} aria-label="关闭提示">
        <X size={15} />
      </button>
    </div>
  );

  if (withinLayer) return notice;

  return (
    <div className={`floating-notice-layer ${scope}`}>
      {notice}
    </div>
  );
}
