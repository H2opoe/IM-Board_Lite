import { releaseBuildEnv } from "./release-build-env.mjs";
import { existsSync } from "node:fs";
import { dirname, resolve, win32 } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";
import { runWithVisualStudio } from "./windows-dev-env.mjs";

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
try {
  if (process.platform !== "win32") throw new Error("此入口需要原生 Windows；交叉编译请使用 tauri:build:windows:xwin。");
  const [command, ...args] = process.argv.slice(2);
  if (!command) throw new Error("missing command");
  const vswhere = win32.join(`${process.env.SystemDrive || "C:"}\\`, "Program Files (x86)", "Microsoft Visual Studio", "Installer", "vswhere.exe");
  if (!existsSync(vswhere)) throw new Error("Visual Studio Installer 的 vswhere.exe 不存在。");
  const result = spawnSync(vswhere, ["-latest", "-products", "*", "-requires", "Microsoft.VisualStudio.Component.VC.Tools.x86.x64", "-property", "installationPath"], { encoding: "utf8" });
  const vsRoot = result.status === 0 ? result.stdout.trim().split(/\r?\n/).find(Boolean) : null;
  if (!vsRoot) throw new Error("Visual Studio Build Tools with C++ x64 tools was not found");
  const vsDevCmd = win32.join(vsRoot, "Common7", "Tools", "VsDevCmd.bat");
  if (!existsSync(vsDevCmd)) throw new Error(`VsDevCmd.bat was not found at ${vsDevCmd}`);
  process.exitCode = runWithVisualStudio({ appRoot, vsDevCmd, command, args, env: args.includes("build") ? releaseBuildEnv(appRoot) : process.env, temporaryRoot: tmpdir() });
} catch (error) {
  console.error(`[windows-env] ${error.message}`);
  process.exitCode = 1;
}
