import { spawnSync } from "node:child_process";

// 只检查构建工具；账号初始化与运行时校验由应用及现有校验命令负责。
const checks = [];
for (const [label, command] of [["Node", process.execPath], ["npm", "npm"], ["Rust cargo", "cargo"], ["Rust compiler", "rustc"]]) {
  const result = spawnSync(command, ["--version"], { encoding: "utf8" });
  checks.push({ label, ok: result.status === 0, detail: result.status === 0 ? (result.stdout || result.stderr).split(/\r?\n/)[0] : `${command} not ready` });
}
const width = Math.max(...checks.map(check => check.label.length));
for (const check of checks) console.log(`${check.ok ? "✓" : "✗"} ${check.label.padEnd(width)} ${check.detail}`);
if (checks.some(check => !check.ok)) process.exitCode = 1;
