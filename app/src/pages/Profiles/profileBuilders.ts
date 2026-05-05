import type { PlatformDeployment } from "../../api/bridgeApi";
import { APP_CACHE_ROOT, APP_SUPPORT_ROOT } from "../../constants/storage";
import type { ImProfile, WechatCandidate } from "../../types";
import { isOriginalWechatCli, joinNativePath } from "./profilePathUtils";

export function buildWechatProfile(
  profile: ImProfile,
  candidate: WechatCandidate,
  sortOrder: number,
  remark: string
): ImProfile {
  const profileId = /^wechat_\d+$/.test(profile.id) ? candidate.id : profile.id;
  const baseDir = candidate.profileDir || `${APP_SUPPORT_ROOT}/Profiles/${profileId}`;
  const cacheDir = candidate.cacheDir || `${APP_CACHE_ROOT}/${profileId}`;
  if (isOriginalWechatCli(candidate)) {
    return {
      ...profile,
      id: profileId,
      label: "微信",
      enabled: true,
      status: "normal",
      sortOrder: profile.sortOrder ?? sortOrder,
      configJson: {
        ...profile.configJson,
        authType: "cli_session",
        remark,
        cliPath: candidate.cliPath || candidate.appPath || "wechat-cli",
        runtime: "windows_original_cli",
        dataDir: candidate.dataDir,
        wechatFilesPath: candidate.wechatFilesPath ?? candidate.dataDir,
        dbDir: candidate.dbDir,
        profileDir: baseDir,
        configDir: baseDir,
        configPath: candidate.configPath || joinNativePath(baseDir, "config.json"),
        keysPath: candidate.keysPath || joinNativePath(baseDir, "all_keys.json"),
        cacheDir,
        tmpDir: joinNativePath(cacheDir, "tmp"),
        cliCommands: {
          listChats: "sessions",
          fetchMessages: "history"
        },
        cliArgs: {
          listChats: {
            limit: "limit"
          },
          fetchMessages: {
            chat: "chat",
            limit: "limit",
            offset: "offset",
            startTime: "start-time",
            endTime: "end-time"
          }
        },
        cliArgPlacement: {
          fetchMessages: {
            chat: "positional"
          }
        }
      },
      updatedAt: new Date().toISOString()
    };
  }
  return {
    ...profile,
    id: profileId,
    label: "微信",
    enabled: true,
    status: "normal",
    sortOrder: profile.sortOrder ?? sortOrder,
    configJson: {
      ...profile.configJson,
      authType: "local_db",
      remark,
      appPath: candidate.appPath,
      bundleId: candidate.bundleId,
      cliPath: candidate.cliPath,
      runtime: candidate.runtime,
      pid: candidate.pid,
      dataDir: candidate.dataDir,
      wechatFilesPath: candidate.wechatFilesPath ?? candidate.dataDir,
      dbDir: candidate.dbDir,
      profileDir: baseDir,
      configPath: candidate.configPath || `${baseDir}/config.json`,
      keysPath: candidate.keysPath || `${baseDir}/all_keys.json`,
      cacheDir,
      tmpDir: `${cacheDir}/tmp`,
      cliCommands: {
        listChats: "sessions",
        fetchMessages: "history"
      },
      cliArgs: {
        listChats: {
          limit: "limit"
        },
        fetchMessages: {
          chat: "chat",
          limit: "limit",
          offset: "offset",
          startTime: "start-time",
          endTime: "end-time"
        }
      },
      cliArgPlacement: {
        fetchMessages: {
          chat: "positional"
        }
      }
    },
    updatedAt: new Date().toISOString()
  };
}

export function buildOriginalWechatDeployment(
  profile: ImProfile,
  candidate: WechatCandidate,
  dbDir: string
): PlatformDeployment {
  const profileId = /^wechat_\d+$/.test(profile.id) ? candidate.id : profile.id;
  const profileDir = candidate.profileDir || `${APP_SUPPORT_ROOT}/Profiles/${profileId}`;
  const cacheDir = candidate.cacheDir || `${APP_CACHE_ROOT}/${profileId}`;
  const tmpDir = joinNativePath(cacheDir, "tmp");
  const configPath = candidate.configPath || joinNativePath(profileDir, "config.json");
  const keysPath = candidate.keysPath || joinNativePath(profileDir, "all_keys.json");
  const cliPath = candidate.cliPath || candidate.appPath || "wechat-cli";
  const effectiveDbDir = dbDir.trim() || candidate.dbDir?.trim();
  const isWindowsPath = cliPath.includes("\\") || profileDir.includes("\\");

  // 同一绑定页会在 macOS 和 Windows 显示命令，路径转义必须按目标终端分别处理。
  const initCommand = isWindowsPath
    ? [
        `New-Item -ItemType Directory -Force -Path ${powershellSingleQuote(profileDir)}, ${powershellSingleQuote(tmpDir)} | Out-Null`,
        `$env:TMPDIR = ${powershellSingleQuote(tmpDir)}`,
        [
          `& ${powershellSingleQuote(cliPath)}`,
          "init",
          "--config",
          powershellSingleQuote(configPath),
          "--keys-file",
          powershellSingleQuote(keysPath),
          effectiveDbDir ? `--db-dir ${powershellSingleQuote(effectiveDbDir)}` : "",
          "--force"
        ]
          .filter(Boolean)
          .join(" ")
      ].join("\n")
    : [
        `mkdir -p ${shellSingleQuote(profileDir)} ${shellSingleQuote(tmpDir)}`,
        `TMPDIR=${shellSingleQuote(tmpDir)} ${shellSingleQuote(cliPath)} init --config ${shellSingleQuote(configPath)} --keys-file ${shellSingleQuote(keysPath)}${effectiveDbDir ? ` --db-dir ${shellSingleQuote(effectiveDbDir)}` : ""} --force`
      ].join("\n");

  return {
    platform: "wechat",
    cliPath,
    configDir: profileDir,
    command: initCommand,
    source: "https://github.com/huohuoer/wechat-cli",
    currentVersion: candidate.cliVersion || "已配置版本"
  };
}

export function buildOriginalWechatCandidate(
  profile: ImProfile,
  deployment: PlatformDeployment
): WechatCandidate {
  const profileId = /^wechat_\d+$/.test(profile.id) ? "wechat_original" : profile.id;
  const profileDir = deployment.configDir || `${APP_SUPPORT_ROOT}/Profiles/${profileId}`;
  const cacheDir = `${APP_CACHE_ROOT}/${profileId}`;
  return {
    id: profileId,
    label: "原版微信",
    pid: 0,
    relatedPids: [],
    bundleId: "wechat-cli-original",
    containerId: "wechat-cli-original",
    appPath: deployment.cliPath,
    cliPath: deployment.cliPath,
    cliVersion: deployment.currentVersion,
    runtime: "windows_original_cli",
    setupMode: "windows_original_cli",
    requiresPassword: false,
    dataDir: "",
    wechatFilesPath: "",
    dbDir: "",
    candidateDbDirs: [],
    running: true,
    confidence: "cli",
    profileDir,
    configPath: joinNativePath(profileDir, "config.json"),
    keysPath: joinNativePath(profileDir, "all_keys.json"),
    cacheDir
  };
}

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
        listChats: "im chats list",
        fetchMessages: "im +chat-messages-list",
        searchMessages: "im +messages-search",
        authStatus: "auth status"
      },
      cliArgs: {
        listChats: {
          pageSize: "page-size"
        },
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

function shellSingleQuote(value: string): string {
  return `'${value.replace(/'/g, "'\\''")}'`;
}

function powershellSingleQuote(value: string): string {
  return `'${value.replace(/'/g, "''")}'`;
}
