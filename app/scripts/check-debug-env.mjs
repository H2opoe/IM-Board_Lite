import { access, constants, stat } from "node:fs/promises";
import { spawnSync } from "node:child_process";
import { join } from "node:path";

const checks = [];

async function exists(label, path, executable = false) {
  try {
    await access(path, executable ? constants.X_OK : constants.R_OK);
    checks.push({ label, ok: true, detail: path });
  } catch {
    checks.push({ label, ok: false, detail: path });
  }
}

function command(label, cmd, args = ["--version"]) {
  const result = spawnSync(cmd, args, { encoding: "utf8" });
  checks.push({
    label,
    ok: result.status === 0,
    detail: result.status === 0 ? (result.stdout || result.stderr).split("\n")[0] : `${cmd} not ready`
  });
}

function firstReadyCommand(label, candidates, args = ["--version"]) {
  for (const cmd of candidates) {
    const result = spawnSync(cmd, args, { encoding: "utf8" });
    if (result.status === 0) {
      checks.push({
        label,
        ok: true,
        detail: `${cmd} · ${(result.stdout || result.stderr).split("\n")[0]}`
      });
      return;
    }
  }
  checks.push({ label, ok: false, detail: candidates.join(" or ") });
}

command("Node", "node", ["--version"]);
command("npm", "npm", ["--version"]);
command("Rust cargo", "cargo", ["--version"]);
firstReadyCommand("wechat query CLI", ["wechat-cli", "/Users/chase/.local/bin/wechat-cli-work"], ["--version"]);
await exists("work wechat-cli wrapper", "/Users/chase/.local/bin/wechat-cli-work", true);
await exists("multi-instance wechat-cli source", "/Users/chase/wechat-cli-work/wechat_cli/main.py");
await exists("local WeChat bridge", join(process.cwd(), "bridges/wechat/bridge_main"), true);

for (const path of ["/Users/chase/.wechat-cli/config.json", "/Users/chase/.wechat-cli/all_keys.json", "/Users/chase/.wechat-cli/config_work.json", "/Users/chase/.wechat-cli/all_keys_work.json"]) {
  try {
    const info = await stat(path);
    checks.push({ label: path.replace("/Users/chase/.wechat-cli/", ""), ok: info.size > 2, detail: `${info.size} bytes` });
  } catch {
    checks.push({ label: path.replace("/Users/chase/.wechat-cli/", ""), ok: false, detail: "missing" });
  }
}

const width = Math.max(...checks.map((check) => check.label.length));
for (const check of checks) {
  console.log(`${check.ok ? "✓" : "✗"} ${check.label.padEnd(width)} ${check.detail}`);
}

if (checks.some((check) => !check.ok)) {
  process.exitCode = 1;
}
