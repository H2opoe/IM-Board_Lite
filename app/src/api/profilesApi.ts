import { invoke } from "@tauri-apps/api/core";
import { platformLabel } from "../constants/platforms";
import { APP_CACHE_ROOT, APP_SUPPORT_ROOT } from "../constants/storage";
import { demoProfiles } from "../demo/demoData";
import type { ImProfile, Platform } from "../types";
import { isDemoMode } from "./demoMode";
import { isTauri, requireTauri } from "./tauri";

export async function listProfiles(): Promise<ImProfile[]> {
  if (isDemoMode()) return [...demoProfiles].sort((left, right) => left.sortOrder - right.sortOrder);
  if (isTauri) return invoke("list_profiles");
  return requireTauri("读取账号配置");
}

export async function upsertProfile(profile: ImProfile): Promise<ImProfile> {
  if (isTauri) return invoke("upsert_profile", { profile });
  return requireTauri("保存账号配置");
}

export async function deleteProfile(profileId: string): Promise<void> {
  if (isTauri) return invoke("delete_profile", { profileId });
  void profileId;
  return requireTauri("删除账号配置");
}

export async function reorderProfiles(profiles: ImProfile[]): Promise<void> {
  const ordered = profiles.map((profile, index) => ({
    ...profile,
    sortOrder: index,
    updatedAt: new Date().toISOString()
  }));
  if (isTauri) {
    await Promise.all(ordered.map((profile) => invoke("upsert_profile", { profile })));
    return;
  }
  return requireTauri("调整账号排序");
}

export function createProfileDraft(platform: Platform, sortOrder = 0): ImProfile {
  const now = new Date().toISOString();
  const id = `${platform}_${Date.now()}`;
  const baseDir = `${APP_SUPPORT_ROOT}/Profiles/${id}`;
  return {
    id,
    platform,
    label: platformLabel(platform),
    enabled: true,
    configJson: {
      authType: platform === "wechat" ? "local_db" : "cli_session",
      configPath: `${baseDir}/config.json`,
      cacheDir: `${APP_CACHE_ROOT}/${id}`,
      ...(platform === "wechat"
        ? {
            keysPath: `${baseDir}/all_keys.json`,
            bundleId: "com.tencent.xinWeChat"
          }
        : platform === "wecom"
          ? {
              authType: "cli_session",
              configDir: `${baseDir}/wecom`,
              cliPath: "wecom-cli"
            }
          : platform === "dingtalk"
            ? {
                authType: "cli_session",
                configDir: `${baseDir}/dingtalk`,
                cliPath: "dws"
              }
            : {})
    },
    status: "normal",
    sortOrder,
    createdAt: now,
    updatedAt: now
  };
}
