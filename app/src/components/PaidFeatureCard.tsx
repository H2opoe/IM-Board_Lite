import { MessageCircle } from "lucide-react";
import contactIconUrl from "../assets/developer-contact.png";

// Presentation only. This card never registers a connector or creates an account.
export function PaidFeatureCard({ onOpenDeveloperContact }: { onOpenDeveloperContact: () => void }) {
  return (
    <div className="bind-card disabled">
      <img src={contactIconUrl} alt="" className="platform-icon-lg" />
      <div className="bind-card-title-row">
        <strong>微信</strong>
        <span className="bind-paid-only-label">付费功能</span>
      </div>
      <div className="bind-card-footer">
        <button className="secondary-button bind-contact-button" onClick={onOpenDeveloperContact}>
          <MessageCircle size={16} />
          添加开发者微信
        </button>
      </div>
    </div>
  );
}
