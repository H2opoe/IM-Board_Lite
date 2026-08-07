import type { PlatformDeployment } from "../../../api/bridgeApi";
import type { ImProfile } from "./types";
import { joinNativePath } from "./profilePathUtils";

export function buildWecomProfile(
  profile: ImProfile,
  sortOrder: number,
  remark: string,
  cliPath: string,
  deployment: PlatformDeployment
): ImProfile {
  const cacheDir = joinNativePath(deployment.configDir, "cache");
  return {
    ...profile,
    label: "企业微信",
    enabled: true,
    status: "normal",
    sortOrder: profile.sortOrder ?? sortOrder,
    configJson: {
      ...profile.configJson,
      authType: "cli_session",
      remark,
      cliPath,
      configDir: deployment.configDir,
      officialSource: deployment.source,
      cliVersion: deployment.currentVersion,
      configPath: joinNativePath(deployment.configDir, "bot.enc"),
      cacheDir,
      tmpDir: joinNativePath(cacheDir, "tmp"),
      cliCommands: {
        listChats: "msg get_msg_chat_list",
        fetchMessages: "msg get_message",
        fetchMedia: "msg get_msg_media"
      },
      timeWindowLimitDays: 7
    },
    updatedAt: new Date().toISOString()
  };
}

export function buildFeishuProfile(
  profile: ImProfile,
  sortOrder: number,
  remark: string,
  cliPath: string,
  deployment: PlatformDeployment
): ImProfile {
  const cacheDir = joinNativePath(deployment.configDir, "cache");
  const feishuHomeDir = joinNativePath(deployment.configDir, "home");
  const feishuConfigDir = joinNativePath(deployment.configDir, ".lark-cli");
  return {
    ...profile,
    label: "飞书",
    enabled: true,
    status: "normal",
    sortOrder: profile.sortOrder ?? sortOrder,
    configJson: {
      ...profile.configJson,
      authType: "cli_session",
      remark,
      cliPath,
      configDir: deployment.configDir,
      homeDir: feishuHomeDir,
      larkConfigDir: feishuConfigDir,
      profileName: profile.id,
      officialSource: deployment.source,
      cliVersion: deployment.currentVersion,
      configPath: joinNativePath(deployment.configDir, "config.json"),
      cacheDir,
      tmpDir: joinNativePath(cacheDir, "tmp"),
      cliCommands: {
        fetchMessages: "im +chat-messages-list",
        searchMessages: "im +messages-search",
        authStatus: "auth status"
      },
      cliArgs: {
        fetchMessages: {
          chat: "container_id",
          startTime: "start_time",
          endTime: "end_time",
          pageSize: "page-size"
        }
      }
    },
    updatedAt: new Date().toISOString()
  };
}

export function buildDingtalkProfile(
  profile: ImProfile,
  sortOrder: number,
  remark: string,
  cliPath: string,
  deployment: PlatformDeployment
): ImProfile {
  const cacheDir = joinNativePath(deployment.configDir, "cache");
  const dingtalkKeychainDir = joinNativePath(deployment.configDir, "keychain");
  return {
    ...profile,
    label: "钉钉",
    enabled: true,
    status: "normal",
    sortOrder: profile.sortOrder ?? sortOrder,
    configJson: {
      ...profile.configJson,
      authType: "cli_session",
      remark,
      cliPath,
      configDir: deployment.configDir,
      dwsCacheDir: cacheDir,
      dwsKeychainDir: dingtalkKeychainDir,
      authIdentity: profile.id,
      tenant: profile.id,
      officialSource: deployment.source,
      cliVersion: deployment.currentVersion,
      configPath: joinNativePath(deployment.configDir, "config.json"),
      cacheDir,
      tmpDir: joinNativePath(cacheDir, "tmp"),
      cliCommands: {
        listChats: "chat list-top-conversations",
        fetchMessages: "chat message search",
        searchMessages: "chat message list-all",
        authStatus: "auth status"
      },
      cliArgs: {
        listChats: {
          limit: "limit"
        },
        fetchMessages: {
          chat: "group",
          startTime: "start",
          endTime: "end",
          limit: "limit"
        }
      }
    },
    updatedAt: new Date().toISOString()
  };
}
