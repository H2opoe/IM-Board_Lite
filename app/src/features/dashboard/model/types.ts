import type { Platform } from "../../profiles/model/types";

export interface DashboardMetric {
  key: string;
  label: string;
  value: number;
  sources: SourceStat[];
}

export interface SourceStat {
  profileId: string;
  platform: Platform;
  platformLabel: string;
  remark: string;
  label: string;
  count: number;
  chats: string[];
}

export interface ActionItem {
  id: string;
  itemType: "task" | "reply" | "attention";
  status: "open" | "done" | "ignored";
  priority: "high" | "medium" | "low";
  title: string;
  description: string;
  suggestedReply?: string;
  profileId: string;
  platform: Platform;
  platformLabel: string;
  platformRemark: string;
  sourceLabel: string;
  chatId: string;
  chatName: string;
  evidenceSummary: string;
  contextIncomplete: boolean;
  carryOver: boolean;
  sourceMessageAt: string;
  lastUpdatedAt: string;
  completedAt?: string;
}

export interface DashboardData {
  day: string;
  metrics: DashboardMetric[];
  replies: ActionItem[];
  tasks: ActionItem[];
  topics: Array<{ title: string; summary: string; count?: number; sourceChats?: Array<{ chatName: string; isGroup: boolean }>; sources?: SourceStat[] }>;
  chatRank: Array<{ chat: string; count: number; sourceLabel?: string }>;
  speakerTop: Array<{ speaker: string; count: number; sourceLabel?: string }>;
  hourlyActivity: Array<{ hour: string; count: number; sources?: SourceStat[] }>;
  messageTypes: Array<{ type: string; count: number; sources?: SourceStat[] }>;
  keywords: Array<{ text: string; weight: number; count?: number; sources?: SourceStat[] }>;
  aiStatus: "not_configured" | "ready" | "analyzing" | "failed";
  syncStatus: string;
}
