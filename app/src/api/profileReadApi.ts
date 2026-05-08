import type { AccountIdentity, DingtalkIdentity, ImProfile } from "../features/profiles/model/types";
import { runBridgeCommand } from "./bridgeApi";
import { isTauri, requireTauri } from "./tauri";

export async function testProfileRead(profile: ImProfile): Promise<string> {
  if (isTauri) {
    if (profile.platform === "wecom") {
      const contacts = await runBridgeCommand(profile, "list-contacts", {});
      const todayChats = await runBridgeCommand(profile, "list-chats", todaySearchArgs("200"));
      return formatReadSuccess(`通讯录${countArray(contacts.data)}人；今天群聊${countGroups(todayChats.data)}个`);
    }
    if (profile.platform === "feishu") {
      const todaySessions = await runBridgeCommand(profile, "search-messages", todaySearchArgs("20"));
      const groups = await runBridgeCommand(profile, "list-chats", { limit: "100" });
      return formatReadSuccess(`今天会话${countArray(todaySessions.data)}个；群聊${countGroups(groups.data)}个`);
    }
    if (profile.platform === "dingtalk") {
      const groups = await searchDingtalkTestGroups(profile);
      const groupList = groups.slice(0, 12);
      let messageCount = 0;
      let todayChatCount = 0;
      for (const group of groupList) {
        const chatId = recordString(group, ["chatId", "openConversationId", "conversationId", "id"]);
        if (!chatId) continue;
        const messages = await runBridgeCommand(profile, "fetch-messages", {
          chat: chatId,
          chat_name: recordString(group, ["chatName", "title", "name"]) || chatId,
          ...todaySearchArgs("50"),
          forward: "true"
        });
        const groupMessageCount = countArray(messages.data);
        messageCount += groupMessageCount;
        if (groupMessageCount > 0) todayChatCount += 1;
        if (messageCount > 0 && todayChatCount >= 3) break;
      }
      return formatReadSuccess(`今天会话${todayChatCount}个；群聊${groups.length}个`);
    }
    const sessions = await runBridgeCommand(profile, "list-chats", { limit: "20" });
    return formatReadSuccess(`最近会话${countArray(sessions.data)}个`);
  }
  return requireTauri("测试账号读取");
}

export async function getDingtalkIdentity(profile: ImProfile): Promise<DingtalkIdentity> {
  if (profile.platform !== "dingtalk") {
    throw new Error("只能读取钉钉账号身份。");
  }
  if (!isTauri) return requireTauri("读取钉钉身份");
  const envelope = await runBridgeCommand(profile, "get-self", {});
  const identity = parseDingtalkIdentity(envelope.data);
  if (!identity.corpId || !identity.userId) {
    throw new Error("未能读取钉钉当前授权身份，请确认绑定命令最后的get-self返回了用户信息。");
  }
  return identity;
}

export async function verifyOfficialCliAuthorization(profile: ImProfile): Promise<void> {
  if (profile.platform !== "wecom" && profile.platform !== "feishu") {
    throw new Error("只能核验企业微信或飞书授权。");
  }
  if (!isTauri) return requireTauri("核验账号授权");
  await runBridgeCommand(profile, "auth-status", {});
}

export async function getOfficialCliAccountIdentity(profile: ImProfile): Promise<AccountIdentity | null> {
  if (profile.platform !== "wecom" && profile.platform !== "feishu") {
    return null;
  }
  if (!isTauri) return requireTauri("读取账号授权身份");
  const envelope = await runBridgeCommand(profile, profile.platform === "wecom" ? "account-identity" : "auth-status", {});
  if (profile.platform === "wecom") return parseWecomIdentity(envelope.data);
  return parseFeishuIdentity(envelope.data);
}

function formatReadSuccess(summary: string) {
  return `测试读取成功：${summary}。`;
}

function todaySearchArgs(limit: string) {
  const now = new Date();
  const start = new Date(now);
  start.setHours(0, 0, 0, 0);
  const end = new Date(now);
  end.setHours(23, 59, 59, 999);
  return {
    start_time: formatLocalDateTime(start),
    end_time: formatLocalDateTime(end),
    limit
  };
}

function countArray(data: unknown) {
  return Array.isArray(data) ? data.length : 0;
}

function arrayData(data: unknown) {
  return Array.isArray(data) ? data : [];
}

function recordString(value: unknown, keys: string[]) {
  if (!value || typeof value !== "object") return "";
  const record = value as Record<string, unknown>;
  const matched = keys.map((key) => record[key]).find((item): item is string => typeof item === "string" && item.trim().length > 0);
  return matched?.trim() ?? "";
}

function recordObject(value: unknown) {
  return value && typeof value === "object" ? (value as Record<string, unknown>) : {};
}

function parseDingtalkIdentity(data: unknown): DingtalkIdentity {
  const records = Array.isArray(data)
    ? data
    : data && typeof data === "object"
      ? arrayData((data as Record<string, unknown>).result).concat(arrayData((data as Record<string, unknown>).data))
      : [];
  const first = records.find((item) => item && typeof item === "object") as Record<string, unknown> | undefined;
  const employee = first?.orgEmployeeModel && typeof first.orgEmployeeModel === "object" ? (first.orgEmployeeModel as Record<string, unknown>) : {};
  return {
    corpId: recordString(employee, ["corpId", "corp_id"]) || recordString(first, ["corpId", "corp_id"]),
    orgName: recordString(employee, ["orgName", "org_name"]),
    userId: recordString(employee, ["userId", "user_id", "unionId", "union_id"]) || recordString(first, ["userId", "user_id", "unionId", "union_id"]),
    userName: recordString(employee, ["orgUserName", "name", "userName"]) || recordString(first, ["name", "userName"])
  };
}

function parseFeishuIdentity(data: unknown): AccountIdentity | null {
  const records = flattenRecords(data);
  for (const record of records) {
    const userId = recordString(record, [
      "userId",
      "user_id",
      "userOpenId",
      "user_open_id",
      "openId",
      "open_id",
      "unionId",
      "union_id",
      "employeeId",
      "employee_id",
      "email"
    ]);
    if (!userId) continue;
    return {
      platform: "feishu",
      tenantId: recordString(record, ["tenantKey", "tenant_key", "tenantId", "tenant_id", "appId", "app_id"]),
      tenantName: recordString(record, ["tenantName", "tenant_name", "enterpriseName", "enterprise_name", "appName", "app_name"]),
      userId,
      userName: recordString(record, ["userName", "user_name", "name", "displayName", "display_name", "email"])
    };
  }
  return null;
}

function parseWecomIdentity(data: unknown): AccountIdentity | null {
  const record = recordObject(data);
  const botId = recordString(record, ["botId", "bot_id", "id"]);
  if (!botId) return null;
  return {
    platform: "wecom",
    tenantId: "",
    tenantName: "企业微信机器人",
    userId: botId,
    userName: recordString(record, ["name", "userName", "user_name"]) || "机器人"
  };
}

function flattenRecords(value: unknown): Array<Record<string, unknown>> {
  const records: Array<Record<string, unknown>> = [];
  const visit = (current: unknown) => {
    if (!current || typeof current !== "object") return;
    if (Array.isArray(current)) {
      current.forEach(visit);
      return;
    }
    const record = current as Record<string, unknown>;
    records.push(record);
    Object.values(record).forEach(visit);
  };
  visit(value);
  return records;
}

async function searchDingtalkTestGroups(profile: ImProfile) {
  const groups: unknown[] = [];
  const seen = new Set<string>();
  for (const query of dingtalkSearchQueries(profile)) {
    const result = await runBridgeCommand(profile, "search-groups", { query });
    for (const group of arrayData(result.data)) {
      const chatId = recordString(group, ["chatId", "chat_id", "openConversationId", "conversationId", "id"]);
      if (!chatId || seen.has(chatId)) continue;
      seen.add(chatId);
      groups.push(group);
    }
  }
  return groups;
}

function dingtalkSearchQueries(profile: ImProfile) {
  const queries: string[] = [];
  for (const item of arrayData(profile.configJson.syncSearchQueries)) {
    if (typeof item === "string") pushDingtalkSearchQuery(queries, item);
  }
  pushDingtalkSearchQuery(queries, recordString(profile.configJson, ["syncSearchQuery"]));
  const identity = recordObject(profile.configJson.accountIdentity);
  for (const key of ["orgName", "corpName", "tenantName", "userName"]) {
    pushDingtalkSearchQuery(queries, recordString(identity, [key]));
  }
  pushDingtalkSearchQuery(queries, recordString(profile.configJson, ["remark"]));
  if (profile.label !== "钉钉") pushDingtalkSearchQuery(queries, profile.label);
  if (queries.length === 0) pushDingtalkSearchQuery(queries, "公司");
  return queries;
}

function pushDingtalkSearchQuery(queries: string[], query: string) {
  const normalized = query.trim();
  if (!normalized || normalized === "钉钉" || queries.includes(normalized)) return;
  queries.push(normalized);
}

function countGroups(data: unknown) {
  if (!Array.isArray(data)) return 0;
  return data.filter((item) => {
    if (!item || typeof item !== "object") return false;
    const record = item as Record<string, unknown>;
    return (
      record.isGroup === true ||
      record.is_group === true ||
      record.chatType === "group" ||
      record.chat_type === "group" ||
      record.chatType === 2 ||
      record.chat_type === 2
    );
  }).length;
}

function formatLocalDateTime(date: Date) {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}
