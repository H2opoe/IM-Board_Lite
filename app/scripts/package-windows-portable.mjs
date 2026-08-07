import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync, statSync } from "node:fs";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import { releaseConfig } from "./release-config.mjs";

const config = releaseConfig();
const repoRoot = join(config.appRoot, "..");
const targetDir = join(config.appRoot, "src-tauri", "target", "x86_64-pc-windows-msvc", "release");
const binaryPath = join(targetDir, "im-board.exe");
const dllPath = join(targetDir, "im_board_lib.dll");
const releaseTemplateDir = join(repoRoot, "artifacts", "tmp", "release-windows-portable", "lite", "IM-Board");
const targetResourceDir = join(targetDir, "_up_");
const stagingRoot = join(repoRoot, "artifacts", "tmp", "windows-portable");
const stagingAppDir = join(stagingRoot, "IM-Board");
const outputDir = join(repoRoot, "artifacts", "windows");
const outputPath = join(outputDir, `IM-Board_${config.releaseLabel}_windows_x64_portable.zip`);

function fail(message) {
  console.error(`[windows-package] ${message}`);
  process.exit(1);
}

function requireFile(path, label) {
  if (!existsSync(path)) {
    fail(`${label} 不存在：${path}`);
  }
}

function removeMacosMetadata(path) {
  for (const entry of readdirSync(path)) {
    const entryPath = join(path, entry);
    if (entry === ".DS_Store") {
      rmSync(entryPath, { force: true });
      continue;
    }
    if (statSync(entryPath).isDirectory()) {
      removeMacosMetadata(entryPath);
    }
  }
}

function assertLiteResources(path) {
  const forbidden = /(^|[\\/])(wechat(?:-cli|-cli-source|_cli|_bridge)?|weixin|bridge_main|wx_key|frida|python)([\\/._-]|$)/i;
  const forbiddenContent = [/wechat/i, /weixin/i, /wx[_-]?key/i, /(^|[^a-z])frida([^a-z]|$)/i, /bridge_main/i, /wxid/i, /filehelper/i, /strcontent/i, /msgsvrid/i, /talker/i, /@chatroom/i, /文件传输助手/u, /拍了拍/u, /(?<!企业)微信/u];
  const pending = [path];
  while (pending.length > 0) {
    const current = pending.pop();
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const entryPath = join(current, entry.name);
      if (forbidden.test(entryPath)) {
        fail(`Lite 包禁止包含微信源码或运行时：${entryPath}`);
      }
      if (entry.isDirectory()) pending.push(entryPath);
      else if (entry.isFile()) {
        const content = readFileSync(entryPath).toString("utf8");
        if (forbiddenContent.some((pattern) => pattern.test(content))) {
          fail(`Lite 包禁止包含微信源码标记：${entryPath}`);
        }
      }
    }
  }
}

requireFile(binaryPath, "Windows 主程序");
requireFile(dllPath, "Windows Rust 动态库");
if (!existsSync(targetResourceDir)) {
  fail(`Windows 打包资源目录不存在：${targetResourceDir}`);
}
if (!existsSync(releaseTemplateDir)) {
  fail(`release portable 模板不存在：${releaseTemplateDir}`);
}

rmSync(stagingRoot, { recursive: true, force: true });
mkdirSync(outputDir, { recursive: true });
cpSync(releaseTemplateDir, stagingAppDir, { recursive: true });
rmSync(join(stagingAppDir, "_up_"), { recursive: true, force: true });
cpSync(targetResourceDir, join(stagingAppDir, "_up_"), { recursive: true });

// release 包提供 Windows portable 目录和内置运行时；这里仅替换本机新编译出的程序文件。
cpSync(binaryPath, join(stagingAppDir, "IM-Board.exe"));
cpSync(dllPath, join(stagingAppDir, "im_board_lib.dll"));
removeMacosMetadata(stagingAppDir);
assertLiteResources(stagingAppDir);

rmSync(outputPath, { force: true });
const result = spawnSync("zip", ["-X", "-q", "-r", outputPath, "IM-Board"], {
  cwd: stagingRoot,
  stdio: "inherit",
});
if (result.status !== 0) {
  fail(`zip 打包失败，退出码 ${result.status}`);
}

const listing = spawnSync("zipinfo", ["-1", outputPath], { encoding: "utf8" });
if (listing.status !== 0) {
  fail(`zip 内容复核失败，退出码 ${listing.status ?? "unknown"}`);
}
for (const path of listing.stdout.split("\n")) {
  if (/(^|[\\/])(wechat(?:-cli|-cli-source|_cli|_bridge)?|weixin|bridge_main|wx_key|frida|python)([\\/]|[._-]|$)/i.test(path)) {
    fail(`Lite zip 禁止包含微信源码或运行时：${path}`);
  }
}

console.log(`[windows-package] ${outputPath}`);
