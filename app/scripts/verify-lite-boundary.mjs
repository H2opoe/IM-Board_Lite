import { existsSync, lstatSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, relative } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const repoRoot = join(appRoot, "..");
const contentPatterns = [
  /wechat/i,
  /weixin/i,
  /wx[_-]?key/i,
  /frida/i,
  /bridge_main/i,
  /wxid/i,
  /filehelper/i,
  /strcontent/i,
  /msgsvrid/i,
  /talker/i,
  /@chatroom/i,
  /文件传输助手/u,
  /拍了拍/u,
  /(?<!企业)微信/u,
];
const pathPatterns = [/wechat/i, /weixin/i, /wx[_-]?key/i, /frida/i, /bridge_main/i, /wxid/i, /filehelper/i];
const sourceRoots = [
  join(appRoot, "src"),
  join(appRoot, "src-tauri", "src"),
  join(appRoot, "src-tauri", "migrations"),
  join(appRoot, "shared"),
];
const sourceFiles = [
  join(appRoot, "package.json"),
  join(appRoot, "src-tauri", "Cargo.toml"),
  join(appRoot, "src-tauri", "build.rs"),
  join(appRoot, "src-tauri", "Info.plist"),
  join(appRoot, "release-manifest.json"),
  join(appRoot, "runtime", "manifest.json"),
  join(appRoot, "src-tauri", "tauri.conf.json"),
  join(appRoot, "src-tauri", "tauri.windows.conf.json"),
].filter(existsSync);
const forbiddenRoots = [
  join(appRoot, "runtime", "python"),
  join(appRoot, "third_party"),
  join(appRoot, "bridges", "wechat"),
];

function fail(violations) {
  console.error("Lite source boundary failed:");
  for (const violation of violations) console.error(`- ${violation}`);
  process.exit(1);
}

function walk(root, output = []) {
  if (!existsSync(root)) return output;
  const info = lstatSync(root);
  if (info.isSymbolicLink()) {
    output.push(root);
    return output;
  }
  if (!info.isDirectory()) {
    output.push(root);
    return output;
  }
  for (const entry of readdirSync(root)) walk(join(root, entry), output);
  return output;
}

function looksBinary(buffer) {
  return buffer.subarray(0, Math.min(buffer.length, 8192)).includes(0);
}

function verifyWorkingTree() {
  const violations = [];
  for (const root of forbiddenRoots) {
    if (walk(root).length > 0) violations.push(`forbidden directory contains files: ${relative(repoRoot, root)}`);
  }
  const files = [...sourceRoots.flatMap((root) => walk(root)), ...sourceFiles];
  for (const file of files) {
    const name = relative(repoRoot, file);
    if (pathPatterns.some((pattern) => pattern.test(name))) {
      violations.push(`forbidden path: ${name}`);
      continue;
    }
    const content = readFileSync(file);
    if (looksBinary(content)) continue;
    const text = content.toString("utf8");
    if (contentPatterns.some((pattern) => pattern.test(text))) {
      violations.push(`forbidden source marker: ${name}`);
    }
  }

  const tracked = spawnSync("git", ["ls-files"], { cwd: repoRoot, encoding: "utf8" });
  if (tracked.status !== 0) violations.push("unable to inspect tracked paths");
  for (const name of tracked.stdout?.split(/\r?\n/) ?? []) {
    if (name && existsSync(join(repoRoot, name)) && pathPatterns.some((pattern) => pattern.test(name))) {
      violations.push(`forbidden tracked path: ${name}`);
    }
  }
  if (violations.length > 0) fail(violations);
}

function verifyReachableHistory() {
  const commits = spawnSync("git", ["rev-list", "--all"], { cwd: repoRoot, encoding: "utf8" });
  if (commits.status !== 0) fail(["unable to enumerate Git history"]);
  const violations = [];
  for (const commit of commits.stdout.split(/\r?\n/).filter(Boolean)) {
    const objects = spawnSync("git", ["ls-tree", "-r", "--name-only", commit], {
      cwd: repoRoot,
      encoding: "utf8",
    });
    for (const name of objects.stdout.split(/\r?\n/).filter(Boolean)) {
      if (pathPatterns.some((pattern) => pattern.test(name))) {
        violations.push(`${commit.slice(0, 12)} forbidden historical path: ${name}`);
      }
    }
    const grep = spawnSync(
      "git",
      ["grep", "-I", "-i", "-E", "wechat|weixin|wx[_-]?key|frida|bridge_main|wxid|filehelper|strcontent|msgsvrid|talker|@chatroom", commit, "--", "app/src", "app/src-tauri/src", "app/src-tauri/migrations", "app/shared"],
      { cwd: repoRoot, encoding: "utf8" },
    );
    if (grep.status === 0) {
      violations.push(`${commit.slice(0, 12)} forbidden historical source content`);
    } else if (grep.status !== 1) {
      violations.push(`${commit.slice(0, 12)} history content scan failed`);
    }
  }
  if (violations.length > 0) fail(violations.slice(0, 100));
}

verifyWorkingTree();
if (process.argv.includes("--history")) verifyReachableHistory();
console.log(`Lite boundary verified${process.argv.includes("--history") ? " including reachable Git history" : ""}.`);
