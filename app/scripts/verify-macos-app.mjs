import { existsSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { releaseConfig } from "./release-config.mjs";

const config = releaseConfig();

const appPath = process.argv[2] ?? join(
  "src-tauri",
  "target",
  "universal-apple-darwin",
  "release",
  "bundle",
  "macos",
  "IM-Board.app",
);

const expected = {
  bundleId: config.bundleId,
  name: config.productName,
  shortVersion: config.appVersion,
  bundleVersion: config.macosBundleVersion,
  releaseLabel: config.releaseLabel,
};

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    ...options,
  });
  return {
    status: result.status,
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    output: `${result.stdout ?? ""}${result.stderr ?? ""}`.trim(),
  };
}

function requireSuccess(command, args, label) {
  const result = run(command, args);
  if (result.status !== 0) {
    throw new Error(`${label} 失败：${result.output}`);
  }
  return result;
}

function readPlistValue(key) {
  const result = requireSuccess(
    "/usr/libexec/PlistBuddy",
    ["-c", `Print :${key}`, join(appPath, "Contents", "Info.plist")],
    `读取 ${key}`,
  );
  return result.stdout.trim();
}

function assertEqual(label, actual, expectedValue) {
  if (actual !== expectedValue) {
    throw new Error(`${label} 不一致：实际为 ${actual}，期望为 ${expectedValue}`);
  }
}

function assertStableShortVersion(label, value) {
  if (!/^\d+\.\d+\.\d+$/.test(value)) {
    throw new Error(`${label} 必须是不带预发布后缀的三段版本号，当前为 ${value}`);
  }
}

function assertNumericBundleVersion(label, value) {
  if (!/^\d+$/.test(value)) {
    throw new Error(`${label} 必须是递增数字构建号，当前为 ${value}`);
  }
}

if (!existsSync(appPath)) {
  throw new Error(`找不到 macOS App：${appPath}`);
}

const plistValues = {
  bundleId: readPlistValue("CFBundleIdentifier"),
  displayName: readPlistValue("CFBundleDisplayName"),
  bundleName: readPlistValue("CFBundleName"),
  shortVersion: readPlistValue("CFBundleShortVersionString"),
  bundleVersion: readPlistValue("CFBundleVersion"),
};

assertEqual("CFBundleIdentifier", plistValues.bundleId, expected.bundleId);
assertEqual("CFBundleDisplayName", plistValues.displayName, expected.name);
assertEqual("CFBundleName", plistValues.bundleName, expected.name);
assertEqual("CFBundleShortVersionString", plistValues.shortVersion, expected.shortVersion);
assertEqual("CFBundleVersion", plistValues.bundleVersion, expected.bundleVersion);
assertStableShortVersion("CFBundleShortVersionString", plistValues.shortVersion);
assertNumericBundleVersion("CFBundleVersion", plistValues.bundleVersion);
if (plistValues.shortVersion.includes("beta") || plistValues.bundleVersion.includes("beta")) {
  throw new Error(`macOS Bundle 版本字段不能包含 beta 标签，发布标签 ${expected.releaseLabel} 只能用于展示和安装包命名`);
}

const carbon = run("/usr/libexec/PlistBuddy", [
  "-c",
  "Print :LSRequiresCarbon",
  join(appPath, "Contents", "Info.plist"),
]);
if (carbon.status === 0) {
  throw new Error("Info.plist 不能包含 LSRequiresCarbon，旧字段会干扰系统按现代 bundle 身份记录权限");
}

const codesignInfo = requireSuccess("codesign", ["-dv", appPath], "读取 codesign 信息");
const identifierMatch = codesignInfo.output.match(/Identifier=(.+)/);
if (!identifierMatch) {
  throw new Error(`codesign 输出缺少 Identifier：${codesignInfo.output}`);
}
assertEqual("codesign Identifier", identifierMatch[1].trim(), expected.bundleId);

requireSuccess("codesign", ["--verify", "--deep", "--strict", "--verbose=4", appPath], "codesign 严格校验");

const spctl = run("spctl", ["--assess", "--type", "execute", "--verbose=4", appPath]);
if (spctl.status !== 0) {
  console.warn(`[macos-app] spctl 未通过：${spctl.output}`);
  console.warn("[macos-app] 如果这是本地 ad-hoc 包，spctl 可能因未使用 Developer ID/notarize 而失败；正式发布包必须修复。");
} else {
  console.log(`[macos-app] spctl 通过：${spctl.output}`);
}

requireSuccess(
  "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister",
  ["-f", appPath],
  "LaunchServices 注册",
);

console.log("[macos-app] IM-Board.app 身份、版本、签名和 LaunchServices 注册检查完成");
