import type { SourceStat } from "../types";

export function sourceStatsTitle(sources?: SourceStat[], emptyText = "暂无平台统计") {
  if (!sources?.length) return emptyText;
  return sources
    .map((source) => {
      const chats = source.chats.length > 0 ? `：${source.chats.join("、")}` : "";
      return `${source.label} ${source.count}${chats}`;
    })
    .join("\n");
}

export function sourceChatsTitle(sources?: SourceStat[], emptyText = "暂无来源会话") {
  if (!sources?.length) return emptyText;
  return sources
    .map((source) => {
      const chats = source.chats.length > 0 ? source.chats.join("、") : "暂无会话名";
      return `${source.label}：${chats}`;
    })
    .join("\n");
}

export function keywordSourcesTitle(sources?: SourceStat[], emptyText = "暂无命中来源") {
  if (!sources?.length) return emptyText;
  return sources.map((source) => `${source.label}：${source.count}次`).join("\n");
}
