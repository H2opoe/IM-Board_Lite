import type { Platform } from "../../features/profiles/model/types";
import dingtalkIcon from "../../assets/platform-icons/dingtalk.png";
import feishuIcon from "../../assets/platform-icons/feishu.png";
import wechatIcon from "../../assets/platform-icons/wechat.png";
import wecomIcon from "../../assets/platform-icons/wecom.png";

const platformIconSrc: Record<Platform, string> = {
  wechat: wechatIcon,
  wecom: wecomIcon,
  feishu: feishuIcon,
  dingtalk: dingtalkIcon
};

const platformAlt: Record<Platform, string> = {
  wechat: "微信",
  wecom: "企业微信",
  feishu: "飞书",
  dingtalk: "钉钉"
};

interface Props {
  platform: Platform;
  className?: string;
}

export function PlatformIcon({ platform, className = "" }: Props) {
  return <img className={`platform-icon ${className}`.trim()} src={platformIconSrc[platform]} alt={platformAlt[platform]} />;
}
