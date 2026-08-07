import type { DashboardData } from "../../dashboard/model/types";

export type SyncJobMode = "incremental" | "full_resync" | "retry_analysis";

export interface SyncProgress {
  profileId: string;
  profileLabel: string;
  phase: string;
  message: string;
  current: number;
  total: number;
  shouldRefreshDashboard: boolean;
}

export interface SyncResult {
  profileId: string;
  syncStatus: string;
  aiStatus: DashboardData["aiStatus"];
  insertedMessages: number;
  analyzedMessages: number;
  warnings: string[];
  startedAt: string;
  finishedAt: string;
}
