import { existsSync, mkdtempSync, mkdirSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { releaseConfig } from "./release-config.mjs";

const config = releaseConfig();

const defaultAppPath = join(
  "src-tauri",
  "target",
  "universal-apple-darwin",
  "release",
  "bundle",
  "macos",
  "IM-Board.app",
);

const args = process.argv.slice(2);
const appPath = resolve(args[0] && !args[0].startsWith("--") ? args[0] : defaultAppPath);
const dmgIndex = args.indexOf("--dmg");
const dmgPath = dmgIndex >= 0 && args[dmgIndex + 1] ? resolve(args[dmgIndex + 1]) : null;
const signingIdentity = process.env.IM_BOARD_MACOS_SIGNING_IDENTITY ?? process.env.APPLE_SIGNING_IDENTITY ?? "-";

function run(command, commandArgs, options = {}) {
  const result = spawnSync(command, commandArgs, {
    encoding: "utf8",
    stdio: options.stdio ?? "pipe",
    ...options,
  });
  return {
    status: result.status,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    output: `${result.stdout ?? ""}${result.stderr ?? ""}`.trim(),
  };
}

function requireSuccess(command, commandArgs, label, options = {}) {
  const result = run(command, commandArgs, options);
  if (result.status !== 0) {
    throw new Error(`${label} 失败：${result.output}`);
  }
  return result;
}

function plistValueExists(key) {
  const result = run("/usr/libexec/PlistBuddy", ["-c", `Print :${key}`, infoPlistPath]);
  return result.status === 0;
}

function deletePlistValue(key) {
  if (!plistValueExists(key)) return false;
  requireSuccess("/usr/libexec/PlistBuddy", ["-c", `Delete :${key}`, infoPlistPath], `删除 ${key}`);
  return true;
}

function setPlistValue(key, value) {
  const action = plistValueExists(key) ? "Set" : "Add";
  const args = action === "Set"
    ? ["-c", `Set :${key} ${value}`, infoPlistPath]
    : ["-c", `Add :${key} string ${value}`, infoPlistPath];
  requireSuccess("/usr/libexec/PlistBuddy", args, `写入 ${key}`);
}

function signApp() {
  const signArgs = ["--force", "--deep", "--sign", signingIdentity];
  if (signingIdentity !== "-") {
    signArgs.push("--options", "runtime");
  }
  signArgs.push(appPath);
  requireSuccess("codesign", signArgs, "重新签名 IM-Board.app", { stdio: "inherit" });
}

function createDmg() {
  if (!dmgPath) return;
  const stageDir = mkdtempSync(join(tmpdir(), "im-board-dmg-"));
  try {
    requireSuccess("ditto", [appPath, join(stageDir, basename(appPath))], "复制修复后的 IM-Board.app");
    symlinkSync("/Applications", join(stageDir, "Applications"));
    mkdirSync(dirname(dmgPath), { recursive: true });
    rmSync(dmgPath, { force: true });
    requireSuccess(
      "hdiutil",
      ["create", "-volname", "IM-Board", "-srcfolder", stageDir, "-ov", "-format", "UDZO", dmgPath],
      "生成修复后的 DMG",
      { stdio: "inherit" },
    );
    requireSuccess("hdiutil", ["verify", dmgPath], "校验修复后的 DMG", { stdio: "inherit" });
  } finally {
    rmSync(stageDir, { recursive: true, force: true });
  }
}

if (!existsSync(appPath)) {
  throw new Error(`找不到 macOS App：${appPath}`);
}

const infoPlistPath = join(appPath, "Contents", "Info.plist");
if (!existsSync(infoPlistPath)) {
  throw new Error(`找不到 Info.plist：${infoPlistPath}`);
}

const removedCarbon = deletePlistValue("LSRequiresCarbon");
if (removedCarbon) {
  console.log("[macos-app] 已从最终 Info.plist 删除 LSRequiresCarbon");
} else {
  console.log("[macos-app] 最终 Info.plist 未包含 LSRequiresCarbon");
}

setPlistValue("CFBundleIdentifier", config.bundleId);
setPlistValue("CFBundleDisplayName", config.productName);
setPlistValue("CFBundleName", config.productName);
setPlistValue("CFBundleShortVersionString", config.appVersion);
setPlistValue("CFBundleVersion", config.macosBundleVersion);

// 修改 Info.plist 后必须重新签名，否则 macOS 仍会按旧封口或损坏签名处理 App 管理授权。
signApp();
createDmg();

console.log(`[macos-app] IM-Board.app 已重新签名：${appPath}`);
if (dmgPath) {
  console.log(`[macos-app] 已生成修复后的 DMG：${dmgPath}`);
}
