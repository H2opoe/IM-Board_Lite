import { spawnSync } from "node:child_process";

const checks = [];

function command(label, cmd, args = ["--version"]) {
  const result = spawnSync(cmd, args, { encoding: "utf8" });
  checks.push({
    label,
    ok: result.status === 0,
    detail: result.status === 0 ? (result.stdout || result.stderr).split("\n")[0] : `${cmd} not ready`
  });
}

command("Node", "node", ["--version"]);
command("npm", "npm", ["--version"]);
command("Rust cargo", "cargo", ["--version"]);
const width = Math.max(...checks.map((check) => check.label.length));
for (const check of checks) {
  console.log(`${check.ok ? "✓" : "✗"} ${check.label.padEnd(width)} ${check.detail}`);
}

if (checks.some((check) => !check.ok)) {
  process.exitCode = 1;
}
