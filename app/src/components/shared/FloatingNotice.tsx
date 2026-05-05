import { type CSSProperties, Children, cloneElement, type PointerEvent, type ReactNode, isValidElement, useEffect, useLayoutEffect, useRef, useState } from "react";
import { AlertCircle, CheckCircle2, Info, X } from "lucide-react";

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

export function FloatingNoticeStack({ children, scope = "page" }: FloatingNoticeStackProps) {
  const notices = Children.toArray(children).filter(Boolean);
  const noticeOrderKey = notices.map((notice, index) => (isValidElement(notice) && notice.key !== null ? notice.key : index)).join("|");
  const [focusIndex, setFocusIndex] = useState(0);
  const [revealedCount, setRevealedCount] = useState(0);
  const lastPointerYRef = useRef<number | null>(null);
  const pointerRemainderRef = useRef(0);

  useEffect(() => {
    setFocusIndex(0);
    setRevealedCount(0);
    lastPointerYRef.current = null;
    pointerRemainderRef.current = 0;
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
  }

  function handlePointerMove(event: PointerEvent<HTMLDivElement>) {
    if (visibleCount <= 1) return;
    const lastPointerY = lastPointerYRef.current ?? event.clientY;
    const deltaY = event.clientY - lastPointerY;
    lastPointerYRef.current = event.clientY;

    const nextRemainder = pointerRemainderRef.current + deltaY;
    if (Math.abs(nextRemainder) < STACK_TURN_DISTANCE_PX) {
      pointerRemainderRef.current = nextRemainder;
      return;
    }

    const direction = nextRemainder > 0 ? 1 : -1;
    setFocusIndex((currentIndex) => (currentIndex + direction + visibleCount) % visibleCount);
    pointerRemainderRef.current = nextRemainder - direction * STACK_TURN_DISTANCE_PX;
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
    >
      <div className="floating-notice-stack">
        {visibleNotices.map(({ index: noticeIndex, notice }, visibleIndex) => (
          <div
            key={isValidElement(notice) && notice.key !== null ? notice.key : noticeIndex}
            className="floating-notice-stack-item"
            style={
              {
                "--notice-index": visibleIndex,
                "--notice-display-index": (visibleIndex - focusIndex + visibleCount) % visibleCount
              } as StackStyle
            }
          >
            {isValidElement<Props>(notice) ? cloneElement(notice, { autoCloseDelayMs: (notices.length - 1 - noticeIndex) * STACK_NOTICE_ENTER_DELAY_MS }) : notice}
          </div>
        ))}
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
  const messageRef = useRef<HTMLSpanElement | null>(null);
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

  useLayoutEffect(() => {
    const element = messageRef.current;
    if (!message || children || !element) {
      setIsSingleLineMessage(false);
      return undefined;
    }

    const measure = () => {
      const style = window.getComputedStyle(element);
      const lineHeight = Number.parseFloat(style.lineHeight) || Number.parseFloat(style.fontSize) * 1.45 || 20;
      const renderedLines = Math.max(1, Math.round(element.scrollHeight / lineHeight));
      setIsSingleLineMessage(renderedLines <= 1);
    };

    measure();
    const resizeObserver = new ResizeObserver(measure);
    resizeObserver.observe(element);
    return () => resizeObserver.disconnect();
  }, [children, message]);

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
          <span ref={messageRef} className="floating-notice-message">
            {message}
          </span>
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
