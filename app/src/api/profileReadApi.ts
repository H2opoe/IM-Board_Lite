import type { AccountIdentity, DingtalkIdentity, ImProfile } from "../features/profiles/model/types";
import { runBridgeCommand } from "./bridgeApi";
import { isTauri, requireTauri } from "./tauri";

export async function testProfileRead(profile: ImProfile): Promise<string> {
  if (isTauri) {
    if (profile.platform === "wecom") {
      const contacts = await runBridgeCommand(profile, "list-contacts", {});
      const todayChats = await runBridgeCommand(profile, "list-chats", todaySearchArgs("200"));
      const warnings = [...(contacts.warnings ?? []), ...(todayChats.warnings ?? [])];
      if (warnings.length) throw new Error(warnings.join("；"));
      return formatReadSuccess(`通讯录${countArray(contacts.data)}人；今天会话${countArray(todayChats.data)}个；群聊${countGroups(todayChats.data)}个`);
    }
    if (profile.platform === "feishu") {
      const todaySessions = await runBridgeCommand(profile, "search-messages", todaySearchArgs("20"));
      return formatReadSuccess(`今天会话${countArray(todaySessions.data)}个；群聊${countGroups(todaySessions.data)}个`);
    }
    if (profile.platform === "dingtalk") {
      const result = await runBridgeCommand(profile, "search-messages", todaySearchArgs("100"));
      if (result.warnings?.length) throw new Error(result.warnings.join("；"));
      const messages = arrayData(result.data);
      if (messages.length === 0) {
        return "钉钉CLI已响应，但当前授权范围内未检索到今天的消息；这不代表钉钉今天没有会话，请核对授权账号和消息读取权限。";
      }
      const chats = new Map<string, unknown>();
      for (const message of messages) {
        const chatId = recordString(message, ["chatId"]);
        if (chatId) chats.set(chatId, message);
      }
      return formatReadSuccess(`今天消息${messages.length}条；会话${chats.size}个；群聊${countGroups([...chats.values()])}个`);
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
  end.setSeconds(end.getSeconds() + 1, 0);
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

function firstObjectField(record: Record<string, unknown>, keys: string[]): Record<string, unknown> {
  for (const key of keys) {
    const value = record[key];
    if (value && typeof value === "object" && !Array.isArray(value)) return value as Record<string, unknown>;
  }
  return {};
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
    const identityRecord = feishuIdentityRecord(record);
    const userId = feishuUserId(identityRecord);
    if (!userId) continue;
    return {
      platform: "feishu",
      tenantId: recordString(identityRecord, ["tenantKey", "tenant_key", "tenantId", "tenant_id", "appId", "app_id"]),
      tenantName: recordString(identityRecord, ["tenantName", "tenant_name", "enterpriseName", "enterprise_name", "appName", "app_name", "brand"]),
      userId,
      userName: recordString(identityRecord, ["userName", "user_name", "name", "displayName", "display_name", "email"])
    };
  }
  return null;
}

function feishuIdentityRecord(record: Record<string, unknown>): Record<string, unknown> {
  const user = firstObjectField(record, ["user", "userInfo", "user_info", "account", "accountInfo", "account_info"]);
  const tenant = firstObjectField(record, ["tenant", "tenantInfo", "tenant_info", "enterprise", "app", "appInfo", "app_info"]);
  // 新版 lark-cli 可能把授权主体和租户信息拆到嵌套对象里；读取身份时合并同一层上下文，
  // 但仍让 user 字段覆盖父级，避免把 appId/name 误当成用户信息。
  return {
    ...record,
    ...tenant,
    ...user
  };
}

function feishuUserId(record: Record<string, unknown>): string {
  return recordString(record, [
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
    "accountId",
    "account_id",
    "uid",
    "email"
  ]);
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
