import { useEffect, useRef, useState } from "react";
import { Check, Copy, X } from "lucide-react";
import wechatIconUrl from "../assets/platform-icons/wechat.png";

interface Props {
  onClose: () => void;
}

const developerWechatId = "DAISUNFILMS";
const developerWechatQrPayload = "https://u.wechat.com/EDsLZ6LQyemJxtrM-PlvT-k?s=3";
const qrQuietZoneSize = 4;
const qrCenterIconSize = 7;
const qrCenterIconPadding = 1;

// 原始二维码内容解码自开发者微信二维码，下面按该内容重新生成纯矢量 QR 矩阵。
const developerWechatQrRows = [
  "111111101000010101010100101111111",
  "100000100100001101100000101000001",
  "101110100100010010111000101011101",
  "101110100110000001110001001011101",
  "101110101111111000100101101011101",
  "100000100100001011000011001000001",
  "111111101010101010101010101111111",
  "000000001110110101111111000000000",
  "100111111111010000111001111010001",
  "001001011111111001101100001001100",
  "010000110011000100100110010000000",
  "101010000100100101000101111011100",
  "100010101011001111001000100010101",
  "001001000100110111101100011000010",
  "000111110101100110001000100110010",
  "101100010011100010100011011010001",
  "111001111110011001101001101101101",
  "110100010011001111000001001101110",
  "100110110101101000100101010110100",
  "111111001000011100010100100111110",
  "001110101111100110110110101010011",
  "111010011001101111111011100101011",
  "010010111010011011000101001000011",
  "100110001111011111100111010000001",
  "000110101011101110111101111110101",
  "000000001100011100100011100011011",
  "111111101000101110101000101011100",
  "100000101000110000110000100011100",
  "101110101000001010001010111111100",
  "101110101110101011101110101010100",
  "101110100100000001000100010001101",
  "100000100010101011110110111100000",
  "111111101001000001000000100010001"
];

async function copyText(text: string) {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand("copy");
  document.body.removeChild(textarea);
}

function DeveloperWechatQr() {
  const qrModulePath = developerWechatQrRows
    .flatMap((row, rowIndex) =>
      Array.from(row).map((cell, columnIndex) => {
        if (cell !== "1") return "";
        return `M${columnIndex + qrQuietZoneSize} ${rowIndex + qrQuietZoneSize}h1v1h-1z`;
      })
    )
    .join("");
  const qrSize = developerWechatQrRows.length + qrQuietZoneSize * 2;
  const qrCenterIconOffset = (qrSize - qrCenterIconSize) / 2;
  const qrCenterIconBackingOffset = qrCenterIconOffset - qrCenterIconPadding;
  const qrCenterIconBackingSize = qrCenterIconSize + qrCenterIconPadding * 2;

  return (
    <svg className="developer-feedback-qr" viewBox={`0 0 ${qrSize} ${qrSize}`} role="img" aria-label="开发者微信二维码">
      <title>{developerWechatQrPayload}</title>
      <rect className="developer-feedback-qr-background" width={qrSize} height={qrSize} rx="2" />
      <path className="developer-feedback-qr-modules" d={qrModulePath} />
      <rect
        className="developer-feedback-qr-icon-backing"
        x={qrCenterIconBackingOffset}
        y={qrCenterIconBackingOffset}
        width={qrCenterIconBackingSize}
        height={qrCenterIconBackingSize}
        rx="2"
      />
      <image
        className="developer-feedback-qr-icon"
        href={wechatIconUrl}
        x={qrCenterIconOffset}
        y={qrCenterIconOffset}
        width={qrCenterIconSize}
        height={qrCenterIconSize}
        preserveAspectRatio="xMidYMid meet"
      />
    </svg>
  );
}

export function DeveloperFeedbackModal({ onClose }: Props) {
  const [copyMessage, setCopyMessage] = useState("");
  const copyTimerRef = useRef<number | null>(null);

  useEffect(() => {
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }

    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("keydown", closeOnEscape);
      if (copyTimerRef.current !== null) {
        window.clearTimeout(copyTimerRef.current);
      }
    };
  }, [onClose]);

  async function copyWechatId() {
    try {
      await copyText(developerWechatId);
      setCopyMessage("已复制");
      if (copyTimerRef.current !== null) {
        window.clearTimeout(copyTimerRef.current);
      }
      copyTimerRef.current = window.setTimeout(() => {
        setCopyMessage("");
        copyTimerRef.current = null;
      }, 1600);
    } catch {
      setCopyMessage("复制失败，请手动复制微信号。");
    }
  }

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <article className="profile-modal developer-feedback-modal" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-header">
          <div>
            <strong>反馈给开发者</strong>
            <span>扫码添加微信，或复制微信号联系</span>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭反馈窗口">
            <X size={17} />
          </button>
        </header>

        <section className="developer-feedback-content">
          <DeveloperWechatQr />
          <div className="developer-feedback-account">
            <span>微信号</span>
            <strong>{developerWechatId}</strong>
            <button className="secondary-button" onClick={copyWechatId}>
              {copyMessage === "已复制" ? <Check size={16} /> : <Copy size={16} />}
              {copyMessage === "已复制" ? "已复制" : "复制微信号"}
            </button>
            {copyMessage && copyMessage !== "已复制" && <p>{copyMessage}</p>}
          </div>
        </section>
      </article>
    </div>
  );
}
