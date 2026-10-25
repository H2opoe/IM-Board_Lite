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
      configPath: joinNativePath(deployment.configDir, "credentials.enc"),
      cacheDir,
      tmpDir: joinNativePath(cacheDir, "tmp"),
      cliCommands: {
        listChats: "chat groups list",
        fetchMessages: "chat messages list",
        fetchMedia: "message files get"
      },
      timeWindowLimitDays: 7
    },
    updatedAt: profile.updatedAt
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
    updatedAt: profile.updatedAt
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
        listChats: "chat +recent-conversations",
        fetchMessages: "chat +chat-messages",
        searchMessages: "chat +search-msg",
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
    updatedAt: profile.updatedAt
  };
}
