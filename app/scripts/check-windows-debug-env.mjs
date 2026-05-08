import { access, constants } from "node:fs/promises";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const appRoot = process.cwd();
const checks = [];

function command(label, cmd, args = ["--version"]) {
  const result = spawnSync(cmd, args, { encoding: "utf8" });
  checks.push({
    label,
    ok: result.status === 0,
    detail: result.status === 0 ? (result.stdout || result.stderr).split(/\r?\n/)[0] : `${cmd} not ready`
  });
}

function whereCommand(label, cmd) {
  const result = spawnSync("where.exe", [cmd], { encoding: "utf8" });
  checks.push({
    label,
    ok: result.status === 0,
    detail: result.status === 0 ? result.stdout.trim().split(/\r?\n/)[0] : `${cmd} not found`
  });
}

async function exists(label, path, executable = false) {
  try {
    await access(path, executable ? constants.X_OK : constants.R_OK);
    checks.push({ label, ok: true, detail: path });
  } catch {
    checks.push({ label, ok: false, detail: path });
  }
}

function installedRustTarget(target) {
  const result = spawnSync("rustup", ["target", "list", "--installed"], { encoding: "utf8" });
  checks.push({
    label: `Rust target ${target}`,
    ok: result.status === 0 && result.stdout.split(/\r?\n/).includes(target),
    detail: target
  });
}

command("Node", "node", ["--version"]);
whereCommand("npm", "npm.cmd");
command("Rust cargo", "cargo", ["--version"]);
command("Rust compiler", "rustc", ["--version"]);
command("Rust toolchain", "rustup", ["show", "active-toolchain"]);
whereCommand("MSVC linker", "link.exe");
command("cargo-xwin", "cargo", ["xwin", "--version"]);
installedRustTarget("x86_64-pc-windows-msvc");

await exists("Tauri CLI", join(appRoot, "node_modules", ".bin", "tauri.cmd"), true);
await exists("WeChat CLI source", join(appRoot, "third_party", "wechat_bridge", "wechat-cli-source", "wechat_cli", "main.py"));
await exists("llama.cpp Windows runtime", join(appRoot, "runtime", "llama.cpp", "b8987", "win-cpu-x64", "llama-b8987", "llama-server.exe"), true);
await exists("Python Windows runtime", join(appRoot, "runtime", "python", "20260414", "win-x64", "python", "python.exe"), true);

checks.push({ label: "CARGO_TARGET_DIR", ok: Boolean(process.env.CARGO_TARGET_DIR), detail: process.env.CARGO_TARGET_DIR || "missing" });

const width = Math.max(...checks.map((check) => check.label.length));
for (const check of checks) {
  console.log(`${check.ok ? "OK  " : "FAIL"} ${check.label.padEnd(width)} ${check.detail}`);
}

if (checks.some((check) => !check.ok)) {
  process.exitCode = 1;
}