import type { DashboardData } from "../../dashboard/model/types";

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
