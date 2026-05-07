import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { CheckCircle2, ChevronLeft, ChevronRight, Clipboard } from "lucide-react";

interface Props {
  text: string;
  className?: string;
  textClassName?: string;
  controlsClassName?: string;
  maxLines?: number;
  element?: "pre" | "code" | "span";
  copyLabel?: string;
  copiedLabel?: string;
  showCopy?: boolean;
  compactCopy?: boolean;
  onSingleLineChange?: (isSingleLine: boolean) => void;
}

const DEFAULT_MAX_LINES = 5;

export function PagedTextBlock({
  text,
  className,
  textClassName,
  controlsClassName,
  maxLines = DEFAULT_MAX_LINES,
  element = "pre",
  copyLabel = "复制全文",
  copiedLabel = "已复制",
  showCopy = true,
  compactCopy = false,
  onSingleLineChange
}: Props) {
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const textRef = useRef<HTMLElement | null>(null);
  const [pageIndex, setPageIndex] = useState(0);
  const [pageCount, setPageCount] = useState(1);
  const [pageHeight, setPageHeight] = useState(0);
  const [isSingleLine, setIsSingleLine] = useState(false);
  const [isCopied, setIsCopied] = useState(false);
  const TextElement = element;

  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    const content = textRef.current;
    if (!viewport || !content) return undefined;

    const measure = () => {
      const style = window.getComputedStyle(content);
      const lineHeight = Number.parseFloat(style.lineHeight) || Number.parseFloat(style.fontSize) * 1.45 || 20;
      const nextPageHeight = Math.ceil(lineHeight * maxLines);
      const nextPageCount = Math.max(1, Math.ceil(content.scrollHeight / nextPageHeight));
      const renderedLines = Math.max(1, Math.round(content.scrollHeight / lineHeight));
      setPageHeight(nextPageHeight);
      setPageCount(nextPageCount);
      setIsSingleLine(renderedLines <= 1);
      onSingleLineChange?.(renderedLines <= 1);
      setPageIndex((current) => Math.min(current, nextPageCount - 1));
    };

    measure();
    const resizeObserver = new ResizeObserver(measure);
    resizeObserver.observe(viewport);
    resizeObserver.observe(content);
    return () => resizeObserver.disconnect();
  }, [maxLines, onSingleLineChange, text]);

  useLayoutEffect(() => {
    if (!viewportRef.current || pageHeight <= 0) return;
    viewportRef.current.scrollTop = pageIndex * pageHeight;
  }, [pageHeight, pageIndex]);

  useEffect(() => {
    setPageIndex(0);
    setIsCopied(false);
  }, [text]);

  async function copyFullText() {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const textarea = document.createElement("textarea");
      textarea.value = text;
      textarea.setAttribute("readonly", "");
      textarea.style.position = "fixed";
      textarea.style.left = "-9999px";
      document.body.append(textarea);
      textarea.select();
      document.execCommand("copy");
      textarea.remove();
    }
    setIsCopied(true);
    window.setTimeout(() => setIsCopied(false), 1_500);
  }

  const hasMultiplePages = pageCount > 1;
  const shouldShowCopy = showCopy && hasMultiplePages;

  return (
    <div className={`paged-text-block ${isSingleLine ? "single-line" : ""} ${className ?? ""}`.trim()}>
      <div
        className="paged-text-viewport"
        ref={viewportRef}
        style={{ maxHeight: pageHeight > 0 ? `${pageHeight}px` : undefined }}
      >
        <TextElement ref={textRef as never} className={textClassName}>
          {text}
        </TextElement>
      </div>
      {(hasMultiplePages || shouldShowCopy) && (
        <div className={`paged-text-controls ${controlsClassName ?? ""}`.trim()}>
          {hasMultiplePages && (
            <>
              <button
                className="icon-button"
                onClick={() => setPageIndex((index) => Math.max(0, index - 1))}
                disabled={pageIndex === 0}
                aria-label="上一页"
              >
                <ChevronLeft size={15} />
              </button>
              <span>
                {pageIndex + 1} / {pageCount}
              </span>
              <button
                className="icon-button"
                onClick={() => setPageIndex((index) => Math.min(pageCount - 1, index + 1))}
                disabled={pageIndex >= pageCount - 1}
                aria-label="下一页"
              >
                <ChevronRight size={15} />
              </button>
            </>
          )}
          {shouldShowCopy && (
            <button
              className={`secondary-button slim-button paged-text-copy ${compactCopy ? "icon-only" : ""}`.trim()}
              onClick={copyFullText}
              aria-label={isCopied ? copiedLabel : copyLabel}
              title={isCopied ? copiedLabel : copyLabel}
            >
              {isCopied ? <CheckCircle2 size={15} /> : <Clipboard size={15} />}
              {!compactCopy && (isCopied ? copiedLabel : copyLabel)}
            </button>
          )}
          {compactCopy && isCopied && <span className="paged-text-copy-feedback">{copiedLabel}</span>}
        </div>
      )}
    </div>
  );
}
