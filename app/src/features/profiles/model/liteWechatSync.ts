import type { ImProfile } from "./types";

export function isLiteUnsupportedWechatProfile(profile: ImProfile): boolean {
  return profile.platform === "wechat";
}
