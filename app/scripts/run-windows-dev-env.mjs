import { existsSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { dirname, isAbsolute, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const appRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repoName = "im-board-win-msvc-x64";
const defaultCargoTargetDir = join(process.env.SystemDrive || "C:", "CodexCargoTarget", repoName);

function fail(message) {
  console.error(`[windows-env] ${message}`);
  process.exit(1);
}

function findVisualStudioRoot() {
  const vswhere = "C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\vswhere.exe";
  if (!existsSync(vswhere)) return null;

  const result = spawnSync(vswhere, [
    "-latest",
    "-products",
    "*",
    "-requires",
    "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
    "-property",
    "installationPath"
  ], { encoding: "utf8" });

  if (result.status !== 0) return null;
  return result.stdout.trim().split(/\r?\n/).find(Boolean) ?? null;
}

function resolveCommand(command) {
  if (isAbsolute(command)) return command;
  const localPath = resolve(appRoot, command);
  if (existsSync(localPath)) return localPath;
  return command;
}

const [command, ...args] = process.argv.slice(2);
if (!command) {
  fail("missing command");
}

const vsRoot = findVisualStudioRoot();
if (!vsRoot) {
  fail("Visual Studio Build Tools with C++ x64 tools was not found");
}

const vsDevCmd = join(vsRoot, "Common7", "Tools", "VsDevCmd.bat");
if (!existsSync(vsDevCmd)) {
  fail(`VsDevCmd.bat was not found at ${vsDevCmd}`);
}

const cargoTargetDir = process.env.CARGO_TARGET_DIR || defaultCargoTargetDir;
const xwinCacheDir = process.env.XWIN_CACHE_DIR || join(appRoot, "..", "artifacts", "cargo-xwin-x64");
const localAppDataDir = join(appRoot, "..", "artifacts", "localappdata");
mkdirSync(cargoTargetDir, { recursive: true });
mkdirSync(join(appRoot, "..", "artifacts", "npm-cache"), { recursive: true });
mkdirSync(join(appRoot, "..", "artifacts", "tmp"), { recursive: true });
mkdirSync(xwinCacheDir, { recursive: true });
mkdirSync(localAppDataDir, { recursive: true });

const resolvedCommand = resolveCommand(command);
const tempDir = join(tmpdir(), "im-board-win");
mkdirSync(tempDir, { recursive: true });
const cmdPath = join(tempDir, `run-${process.pid}.cmd`);
const quotedArgs = args.map((arg) => `"${arg.replace(/"/g, '""')}"`).join(" ");

writeFileSync(cmdPath, [
  "@echo off",
  `call "${vsDevCmd}" -arch=x64 -host_arch=x64 >nul`,
  "if errorlevel 1 exit /b %errorlevel%",
  `set "CARGO_TARGET_DIR=${cargoTargetDir}"`,
  `set "RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc"`,
  `set "XWIN_CACHE_DIR=${xwinCacheDir}"`,
  `set "XWIN_ARCH=x86_64"`,
  `set "IM_BOARD_BRIDGE_ROOT=${join(appRoot, "bridges")}"`,
  `set "npm_config_cache=${join(appRoot, "..", "artifacts", "npm-cache")}"`,
  `set "TMP=${join(appRoot, "..", "artifacts", "tmp")}"`,
  `set "TEMP=${join(appRoot, "..", "artifacts", "tmp")}"`,
  `set "LOCALAPPDATA=${localAppDataDir}"`,
  `echo [windows-env] CARGO_TARGET_DIR=${cargoTargetDir}`,
  `echo [windows-env] XWIN_CACHE_DIR=${xwinCacheDir}`,
  `echo [windows-env] command=${resolvedCommand} ${args.join(" ")}`,
  `"${resolvedCommand}" ${quotedArgs}`,
  "exit /b %errorlevel%"
].join("\r\n"));

const result = spawnSync("cmd.exe", ["/d", "/c", cmdPath], {
  cwd: appRoot,
  stdio: "inherit",
  env: process.env
});

rmSync(cmdPath, { force: true });
process.exit(result.status ?? 1);
