import { invoke } from "@tauri-apps/api/core";
import { platformLabel } from "../../../constants/platforms";
import { demoProfiles } from "../../../demo/demoData";
import { isDemoMode } from "../../../api/demoMode";
import { isTauri, requireTauri } from "../../../api/tauri";
import type { ConnectorCapability, ImProfile, Platform } from "../model/types";

export async function getConnectorCapabilities(): Promise<ConnectorCapability[]> {
  if (isTauri) return invoke("get_connector_capabilities");
  return requireTauri("读取平台能力");
}

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
  if (isTauri) {
    await invoke("reorder_profiles", { profileIds: profiles.map((profile) => profile.id) });
    return;
  }
  return requireTauri("调整账号排序");
}

export async function createProfileDraft(platform: Platform, sortOrder = 0): Promise<ImProfile> {
  if (isTauri) return invoke("create_profile_draft", { platform, sortOrder });
  if (!isDemoMode()) return requireTauri("创建账号配置");
  const now = new Date().toISOString();
  const id = `${platform}_${Date.now()}`;
  return {
    id,
    platform,
    label: platformLabel(platform),
    enabled: true,
    configJson: {
      authType: "cli_session",
      ...(platform === "wecom"
        ? {
            cliPath: "wecom-cli"
          }
        : platform === "dingtalk"
          ? {
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
