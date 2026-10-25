import { openAboutLink } from "../api/aboutLinks";
import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { displayAppVersion, getAppVersion } from "../api/appVersion";
import { THIRD_PARTY_NOTICES } from "../content/THIRD_PARTY_NOTICES";
import { AppIcon } from "./shared/AppIcon";

interface Props {
  onClose: () => void;
}

const authorInfo = {
  productName: "IM-Board·聊天汇总看板（Lite版）",
  authorName: "李俊彦",
  xiaohongshu: "@李俊彦的导演笔记",
  xiaohongshuId: "chasingup",
  contact: "chase_li@qq.com",
  xiaohongshuUrl: "https://www.xiaohongshu.com/user/profile/5bed9e4201e65d00013a32bf",
  copyright: "Copyright © 2026 佛山市戴胜文化传媒有限公司"
};

export function AboutModal({ onClose }: Props) {
  const [linkError, setLinkError] = useState("");
  const [appVersion, setAppVersion] = useState<string | null>(null);

  useEffect(() => {
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }

    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose]);

  useEffect(() => {
    let ignore = false;

    getAppVersion()
      .then((version) => {
        if (!ignore) setAppVersion(version);
      });

    return () => {
      ignore = true;
    };
  }, []);

  function openLink(event: { preventDefault: () => void }, target: "profile" | "email") {
    setLinkError("");
    void openAboutLink(event, target).catch(() => {
      setLinkError(target === "email"
        ? "无法打开邮件应用，请复制邮箱地址，并在系统中设置默认邮件应用。"
        : "无法打开浏览器，请复制链接后在浏览器中访问。");
    });
  }

  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <article className="profile-modal about-modal" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-header">
          <div>
            <strong>关于 {authorInfo.productName}</strong>
            <span>作者信息与开源许可</span>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭关于窗口">
            <X size={17} />
          </button>
        </header>

        <section className="about-section">
          <AppIcon className="about-app-mark" />
          <div>
            <h2>{authorInfo.productName}</h2>
            <p>作者：{authorInfo.authorName}</p>
            <p>
              小红书：
              <a href={authorInfo.xiaohongshuUrl} target="_blank" rel="noreferrer" onClick={(event) => openLink(event, "profile")}>
                {authorInfo.xiaohongshu}（小红书号：{authorInfo.xiaohongshuId}）
              </a>
            </p>
            <p>
              邮箱：
              <a href={`mailto:${authorInfo.contact}`} onClick={(event) => openLink(event, "email")}>{authorInfo.contact}</a>
            </p>
            {linkError && <p role="alert">{linkError}</p>}
            <p>Version {displayAppVersion(appVersion)}</p>
            <p>{authorInfo.copyright}</p>
          </div>
        </section>

        <section className="about-section compact-about-section">
          <strong>调用的开源组件</strong>
          <ul className="license-summary-list">
            <li>企业微信：@wecom/cli（MIT）</li>
            <li>飞书：@larksuite/cli（MIT）</li>
            <li>钉钉：@DingTalk-Real-AI/dingtalk-workspace-cli（Apache-2.0）</li>
          </ul>
        </section>

        <section className="notice-section">
          <strong>THIRD_PARTY_NOTICES</strong>
          <pre>{THIRD_PARTY_NOTICES}</pre>
        </section>
      </article>
    </div>
  );
}
