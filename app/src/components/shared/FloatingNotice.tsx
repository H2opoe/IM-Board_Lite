import { type ReactNode, useEffect, useLayoutEffect, useRef, useState } from "react";
import { AlertCircle, CheckCircle2, Info, X } from "lucide-react";

export type FloatingNoticeScope = "page" | "modal";
export type FloatingNoticeVariant = "info" | "success" | "error";

interface Props {
  message?: string;
  children?: ReactNode;
  variant?: FloatingNoticeVariant;
  scope?: FloatingNoticeScope;
  withinLayer?: boolean;
  autoCloseMs?: number | false;
  onClose: () => void;
}

const SUCCESS_AUTO_CLOSE_MS = 5_000;

export function FloatingNotice({
  message,
  children,
  variant = "info",
  scope = "page",
  withinLayer = false,
  autoCloseMs,
  onClose
}: Props) {
  const onCloseRef = useRef(onClose);
  const messageRef = useRef<HTMLSpanElement | null>(null);
  const [isSingleLineMessage, setIsSingleLineMessage] = useState(false);
  const hasContent = Boolean(message || children);
  const effectiveAutoCloseMs = autoCloseMs === false ? undefined : typeof autoCloseMs === "number" ? autoCloseMs : variant === "success" ? SUCCESS_AUTO_CLOSE_MS : undefined;

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  useEffect(() => {
    if (!hasContent || !effectiveAutoCloseMs) return undefined;
    const timer = window.setTimeout(() => {
      onCloseRef.current();
    }, effectiveAutoCloseMs);
    return () => window.clearTimeout(timer);
  }, [effectiveAutoCloseMs, hasContent, message, variant]);

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
      <button className="floating-notice-close" onClick={onClose} aria-label="关闭提示">
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
