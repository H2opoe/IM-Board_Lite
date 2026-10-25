import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve, win32 } from "node:path";
import { spawnSync } from "node:child_process";

export function cargoTargetDirectory(appRoot, env) {
  if (env.CARGO_TARGET_DIR) return resolve(appRoot, env.CARGO_TARGET_DIR);
  const identity = createHash("sha256").update(appRoot).digest("hex").slice(0, 16);
  return win32.join(`${env.SystemDrive || "C:"}\\`, "CodexCargoTarget", `imboard-${identity}`);
}

export function visualStudioBatch(path) {
  // CALL expands its input twice. Application arguments never enter this batch.
  if (/["%\r\n\0]/.test(path)) throw new Error("Visual Studio 路径含不支持的批处理字符。");
  return ["@echo off", "setlocal DisableDelayedExpansion", `call "${path}" -arch=x64 -host_arch=x64 >nul`, "if errorlevel 1 exit /b %errorlevel%", "set"].join("\r\n");
}

export function environmentFromOutput(output, overrides) {
  const values = new Map();
  for (const line of output.split(/\r?\n/)) {
    const separator = line.indexOf("=");
    if (separator > 0) values.set(line.slice(0, separator).toUpperCase(), [line.slice(0, separator), line.slice(separator + 1)]);
  }
  if (!values.has("PATH")) throw new Error("Visual Studio 未返回有效 PATH。");
  for (const [key, value] of Object.entries(overrides)) values.set(key.toUpperCase(), [key, value]);
  return Object.fromEntries(values.values());
}

export function runWithVisualStudio({ appRoot, vsDevCmd, command, args, env = process.env, temporaryRoot, spawn = spawnSync }) {
  const cargoTargetDir = cargoTargetDirectory(appRoot, env);
  const repoRoot = resolve(appRoot, "..");
  const dirs = {
    CARGO_TARGET_DIR: cargoTargetDir,
    XWIN_CACHE_DIR: env.XWIN_CACHE_DIR || join(repoRoot, "artifacts", "cargo-xwin-x64"),
    npm_config_cache: join(repoRoot, "artifacts", "npm-cache"),
    TMP: join(repoRoot, "artifacts", "tmp"),
    TEMP: join(repoRoot, "artifacts", "tmp"),
    LOCALAPPDATA: join(repoRoot, "artifacts", "localappdata"),
  };
  const resolvedCommand = command === "node" ? process.execPath : existsSync(resolve(appRoot, command)) ? resolve(appRoot, command) : command;
  if (/\.(?:cmd|bat)$/i.test(resolvedCommand)) throw new Error("请通过 node 调用 CLI 的 JavaScript 入口，或使用原生可执行文件。");
  const batch = visualStudioBatch(vsDevCmd);
  for (const path of new Set(Object.values(dirs))) mkdirSync(path, { recursive: true });
  const owned = mkdtempSync(join(temporaryRoot, "imboard-vs-env-"));
  try {
    const path = join(owned, "environment.cmd");
    if (/["%\r\n\0]/.test(path)) throw new Error("临时目录含不支持的批处理字符。");
    writeFileSync(path, batch, { flag: "wx", mode: 0o600 });
    const setup = spawn("cmd.exe", ["/d", "/s", "/c", `""${path}""`], { cwd: appRoot, env, encoding: "utf8", maxBuffer: 8 * 1024 * 1024 });
    if (setup.error) throw setup.error;
    if (setup.status !== 0) throw new Error(`Visual Studio 环境初始化失败，退出码 ${setup.status ?? "unknown"}：${setup.stderr || ""}`);
    const buildEnv = environmentFromOutput(setup.stdout, { ...dirs, RUSTUP_TOOLCHAIN: "stable-x86_64-pc-windows-msvc", XWIN_ARCH: "x86_64" });
    console.log(`[windows-env] CARGO_TARGET_DIR=${cargoTargetDir}`);
    // Project paths and arguments reach the native process without CMD parsing.
    const result = spawn(resolvedCommand, args, { cwd: appRoot, env: buildEnv, stdio: "inherit" });
    if (result.error) throw result.error;
    return result.status ?? 1;
  } finally {
    rmSync(owned, { recursive: true, force: true });
  }
}
