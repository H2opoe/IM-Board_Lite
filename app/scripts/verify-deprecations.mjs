import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const registry = JSON.parse(readFileSync(join(appRoot, "deprecations.json"), "utf8"));
const packageJson = JSON.parse(readFileSync(join(appRoot, "package.json"), "utf8"));

if (registry.schemaVersion !== 1 || !Array.isArray(registry.entries)) {
  throw new Error("deprecations.json 格式无效。");
}

const ids = new Set();
for (const entry of registry.entries) {
  for (const field of ["id", "purpose", "introducedIn", "removeAfter", "owner", "replacement", "verification"]) {
    if (typeof entry[field] !== "string" || !entry[field].trim()) {
      throw new Error(`弃用登记 ${entry.id ?? "<unknown>"} 缺少 ${field}。`);
    }
  }
  if (ids.has(entry.id)) throw new Error(`弃用登记 ID 重复：${entry.id}`);
  ids.add(entry.id);
  if (compareVersions(entry.removeAfter, entry.introducedIn) <= 0) {
    throw new Error(`${entry.id} 的删除版本必须晚于引入版本。`);
  }
  if (compareVersions(packageJson.version, entry.removeAfter) > 0) {
    throw new Error(`${entry.id} 已超过计划删除版本 ${entry.removeAfter}，必须评估并清理。`);
  }
}

console.log(`Deprecated compatibility registry verified: ${registry.entries.length} entries.`);

function compareVersions(left, right) {
  const parse = (value) => {
    if (!/^\d+\.\d+\.\d+$/.test(value)) throw new Error(`无效版本号：${value}`);
    return value.split(".").map(Number);
  };
  const a = parse(left);
  const b = parse(right);
  for (let index = 0; index < 3; index += 1) {
    if (a[index] !== b[index]) return a[index] - b[index];
  }
  return 0;
}
