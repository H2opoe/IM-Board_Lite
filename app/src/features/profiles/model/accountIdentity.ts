import { getDingtalkIdentity, getOfficialCliAccountIdentity } from "../../../api/profileReadApi";
import type { AccountIdentity, ImProfile, Platform } from "./types";

type SupportedIdentityPlatform = Extract<Platform, "wechat" | "wecom" | "feishu" | "dingtalk">;

export async function readBoundAccountIdentity(profile: ImProfile): Promise<AccountIdentity | null> {
  if (profile.platform === "wechat") {
    return readWechatAccountIdentity(profile);
  }
  if (profile.platform === "dingtalk") {
    const identity = await getDingtalkIdentity(profile);
    return {
      platform: "dingtalk",
      tenantId: identity.corpId,
      tenantName: identity.orgName,
      userId: identity.userId,
      userName: identity.userName
    };
  }
  if (profile.platform === "wecom" || profile.platform === "feishu") {
    return getOfficialCliAccountIdentity(profile);
  }
  return null;
}

export async function findDuplicateAccountIdentity(
  platform: SupportedIdentityPlatform,
  identity: AccountIdentity,
  currentProfileId: string,
  profiles: ImProfile[]
): Promise<ImProfile | undefined> {
  for (const profile of profiles) {
    if (profile.id === currentProfileId || profile.platform !== platform) continue;
    const saved = accountIdentityFromProfile(profile);
    if (saved && isSameAccountIdentity(saved, identity)) return profile;
    if (saved) continue;
    try {
      const live = await readBoundAccountIdentity(profile);
      if (live && isSameAccountIdentity(live, identity)) return profile;
    } catch {
      // 旧账号可能缺少身份缓存或授权已失效；不能因此阻断当前账号保存。
    }
  }
  return undefined;
}

export function formatAccountIdentity(identity: AccountIdentity): string {
  return [identity.tenantName, identity.userName, identity.userId].filter(Boolean).join(" / ");
}

export function withAccountIdentity(profile: ImProfile, identity: AccountIdentity): ImProfile {
  return {
    ...profile,
    configJson: {
      ...profile.configJson,
      accountIdentity: {
        platform: identity.platform,
        tenantId: identity.tenantId,
        tenantName: identity.tenantName,
        userId: identity.userId,
        userName: identity.userName
      }
    }
  };
}

function accountIdentityFromProfile(profile: ImProfile): AccountIdentity | null {
  const value = profile.configJson.accountIdentity;
  if (!value || typeof value !== "object") return null;
  const record = value as Record<string, unknown>;
  const userId = stringField(record, "userId");
  if (!userId) return null;
  return {
    platform: profile.platform as SupportedIdentityPlatform,
    tenantId: stringField(record, "tenantId") || stringField(record, "corpId"),
    tenantName: stringField(record, "tenantName") || stringField(record, "orgName"),
    userId,
    userName: stringField(record, "userName")
  };
}

function readWechatAccountIdentity(profile: ImProfile): AccountIdentity | null {
  const dbDir = stringField(profile.configJson, "dbDir");
  const profileDir = stringField(profile.configJson, "profileDir");
  const userId = dbDir || profileDir || profile.id.trim();
  if (!userId) return null;
  return {
    platform: "wechat",
    tenantId: "",
    tenantName: "微信",
    userId,
    userName: stringField(profile.configJson, "remark") || profile.label
  };
}

function isSameAccountIdentity(left: AccountIdentity, right: AccountIdentity): boolean {
  if (left.platform !== right.platform) return false;
  if (left.tenantId && right.tenantId && left.tenantId !== right.tenantId) return false;
  return left.userId === right.userId;
}

function stringField(record: Record<string, unknown>, key: string): string {
  const value = record[key];
  return typeof value === "string" ? value.trim() : "";
}
