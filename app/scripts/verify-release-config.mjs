import { readFileSync } from "node:fs";
import { join } from "node:path";

import { releaseConfig } from "./release-config.mjs";

const config = releaseConfig();
const releaseManifest = JSON.parse(readFileSync(join(config.appRoot, "release-manifest.json"), "utf8"));
const packageJson = JSON.parse(readFileSync(join(config.appRoot, "package.json"), "utf8"));
const packageLock = JSON.parse(readFileSync(join(config.appRoot, "package-lock.json"), "utf8"));
const tauriConfig = JSON.parse(readFileSync(join(config.appRoot, "src-tauri", "tauri.conf.json"), "utf8"));
const cargoToml = readFileSync(join(config.appRoot, "src-tauri", "Cargo.toml"), "utf8");
const cargoVersion = cargoToml.match(/^version\s*=\s*"([^"]+)"/m)?.[1];

const checks = [
  ["release-manifest.json schemaVersion", releaseManifest.schemaVersion, 1],
  ["package.json version", packageJson.version, config.appVersion],
  ["package-lock.json version", packageLock.version, config.appVersion],
  ["package-lock.json root package version", packageLock.packages?.[""]?.version, config.appVersion],
  ["Cargo.toml version", cargoVersion, config.appVersion],
  ["tauri.conf.json version", tauriConfig.version, config.appVersion]
];
if (config.edition === "lite" && !config.releaseLabel.endsWith("-lite")) {
  throw new Error("Lite 版本发布标签必须以 -lite 结尾。");
}
if (!new Set(["full", "lite", "demo"]).has(config.edition)) {
  throw new Error(`release-manifest.json edition 无效：${config.edition}`);
}
if (!new Set(["macos", "windows"]).has(config.target)) {
  throw new Error(`release-manifest.json target 无效：${config.target}`);
}
const tauriMacosBundleVersion = tauriConfig.bundle?.macOS?.bundleVersion;
if (tauriMacosBundleVersion !== undefined) {
  checks.push(["tauri.conf.json macOS bundleVersion", tauriMacosBundleVersion, config.macosBundleVersion]);
}

const mismatches = checks.filter(([, actual, expected]) => actual !== expected);
if (mismatches.length > 0) {
  const detail = mismatches
    .map(([label, actual, expected]) => `${label}: 当前 ${String(actual)}，应为 ${expected}`)
    .join("\n");
  throw new Error(`发布版本元数据不一致：\n${detail}`);
}

console.log(`发布版本元数据一致：${config.releaseLabel} (app ${config.appVersion}, macOS build ${config.macosBundleVersion})`);
