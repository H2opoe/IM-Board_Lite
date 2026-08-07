import { ExternalLink, X } from "lucide-react";

interface Props {
  onClose: () => void;
}

const feedbackUrl = "https://github.com/H2opoe/IM-Board_Lite/issues";

export function DeveloperFeedbackModal({ onClose }: Props) {
  return (
    <div className="modal-backdrop" onMouseDown={onClose}>
      <article className="profile-modal developer-feedback-modal" onMouseDown={(event) => event.stopPropagation()}>
        <header className="modal-header">
          <div>
            <strong>联系开发者</strong>
            <span>通过公开问题反馈页提交建议或故障信息</span>
          </div>
          <button className="icon-button" onClick={onClose} aria-label="关闭反馈窗口">
            <X size={17} />
          </button>
        </header>

        <section className="developer-feedback-content">
          <div className="developer-feedback-account">
            <span>GitHub Issues</span>
            <strong>IM-Board Lite</strong>
            <button className="secondary-button" onClick={() => window.open(feedbackUrl, "_blank", "noopener,noreferrer")}>
              <ExternalLink size={16} />
              打开反馈页
            </button>
          </div>
        </section>
      </article>
    </div>
  );
}
