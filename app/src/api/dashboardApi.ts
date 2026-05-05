import { invoke } from "@tauri-apps/api/core";
import { demoDashboard, updateDemoAction } from "../demo/demoData";
import type { DashboardData } from "../types";
import { isDemoMode } from "./demoMode";
import { isTauri, requireTauri } from "./tauri";

export async function getDashboard(profileId?: string): Promise<DashboardData> {
  if (isDemoMode()) return demoDashboard(profileId);
  if (isTauri) return invoke("get_dashboard", { profileId });
  return requireTauri("读取看板数据");
}

export async function markActionItem(actionId: string, status: "open" | "done" | "ignored") {
  if (isDemoMode()) {
    updateDemoAction(actionId, status);
    return;
  }
  if (isTauri) return invoke("mark_action_item", { actionId, status });
  return requireTauri("更新事项状态");
}
