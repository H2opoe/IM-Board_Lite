import { lstatSync, mkdirSync, mkdtempSync, renameSync, rmSync, symlinkSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { spawnSync } from "node:child_process";

function command(run, name, args) {
  const result = run(name, args, { encoding: "utf8" });
  if (result.error || result.status !== 0) throw new Error(`${name} 失败：${result.error?.message || `${result.stdout || ""}${result.stderr || ""}`.trim() || result.status}`);
  return result;
}

export function repairMacosBundle({ appPath, config, signingIdentity = "-" }, { run = spawnSync, rename = renameSync } = {}) {
  if (!lstatSync(appPath).isDirectory() || !lstatSync(join(appPath, "Contents/Info.plist")).isFile()) throw new Error(`macOS App 目录或 Info.plist 无效：${appPath}`);
  const owned = mkdtempSync(join(dirname(appPath), ".imboard-app-repair-"));
  let preserveBackup = false;
  try {
    const staged = join(owned, basename(appPath));
    command(run, "/usr/bin/ditto", [appPath, staged]);
    const plist = join(staged, "Contents/Info.plist");
    function has(key) { return run("/usr/libexec/PlistBuddy", ["-c", `Print :${key}`, plist], { encoding: "utf8" }).status === 0; }
    if (has("LSRequiresCarbon")) command(run, "/usr/libexec/PlistBuddy", ["-c", "Delete :LSRequiresCarbon", plist]);
    for (const [key, value] of Object.entries({ CFBundleIdentifier: config.bundleId, CFBundleDisplayName: config.productName, CFBundleName: config.productName, CFBundleShortVersionString: config.appVersion, CFBundleVersion: config.macosBundleVersion })) {
      command(run, "/usr/libexec/PlistBuddy", ["-c", has(key) ? `Set :${key} ${value}` : `Add :${key} string ${value}`, plist]);
    }
    // Updating the outer seal keeps nested signatures, executable modes, and each
    // nested binary's own entitlements. Recursive signing would replace them.
    const args = ["--force", "--sign", signingIdentity, "--identifier", config.bundleId, "--preserve-metadata=entitlements,flags,runtime"];
    if (signingIdentity !== "-") args.push("--options", "runtime");
    command(run, "codesign", [...args, staged]);
    command(run, "codesign", ["--verify", "--deep", "--strict", "--verbose=4", staged]);
    const previous = join(owned, "previous.app");
    rename(appPath, previous);
    try {
      rename(staged, appPath);
    } catch (error) {
      try { rename(previous, appPath); }
      catch (rollback) { preserveBackup = true; throw new Error(`发布修复 App 失败：${error.message}；原 App 保留在 ${previous}，恢复失败：${rollback.message}`); }
      throw error;
    }
  } finally {
    if (!preserveBackup) rmSync(owned, { recursive: true, force: true });
  }
}

export function createVerifiedDmg({ appPath, dmgPath, productName }, { run = spawnSync } = {}) {
  mkdirSync(dirname(dmgPath), { recursive: true });
  const owned = mkdtempSync(join(dirname(dmgPath), ".imboard-dmg-"));
  try {
    const payload = join(owned, "payload");
    mkdirSync(payload);
    command(run, "/usr/bin/ditto", [appPath, join(payload, basename(appPath))]);
    symlinkSync("/Applications", join(payload, "Applications"));
    const candidate = join(owned, "candidate.dmg");
    command(run, "hdiutil", ["create", "-volname", productName, "-srcfolder", payload, "-format", "UDZO", candidate]);
    command(run, "hdiutil", ["verify", candidate]);
    renameSync(candidate, dmgPath);
  } finally {
    rmSync(owned, { recursive: true, force: true });
  }
}
