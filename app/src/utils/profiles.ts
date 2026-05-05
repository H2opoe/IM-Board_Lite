import type { ImProfile } from "../types";
import { platformLabel } from "../constants/platforms";

export function profileRemark(profile: ImProfile) {
  const value = profile.configJson.remark;
  return typeof value === "string" ? value.trim() : "";
}

export function profileDisplayName(profile: ImProfile) {
  const remark = profileRemark(profile);
  const label = platformLabel(profile.platform) ?? profile.label;
  return remark ? `${label}·${remark}` : label;
}
