import { readFileSync, readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join } from "node:path";
import { releaseConfig } from "./release-config.mjs";

const config = releaseConfig();

function run(command, args) {
  const result = spawnSync(command, args, {
    cwd: config.appRoot,
    stdio: "inherit",
    env: process.env,
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed`);
  }
}

function assertLiteResources(appPath) {
  const forbidden = /(^|[\\/])(wechat(?:-cli|-cli-source|_cli|_bridge)?|weixin|bridge_main|wx_key|frida|python)([\\/._-]|$)/i;
  const forbiddenContent = [/wechat/i, /weixin/i, /wx[_-]?key/i, /(^|[^a-z])frida([^a-z]|$)/i, /bridge_main/i, /wxid/i, /filehelper/i, /strcontent/i, /msgsvrid/i, /talker/i, /@chatroom/i, /文件传输助手/u, /拍了拍/u, /(?<!企业)微信/u];
  const pending = [join(appPath, "Contents")];
  while (pending.length > 0) {
    const current = pending.pop();
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const entryPath = join(current, entry.name);
      if (forbidden.test(entryPath)) {
        throw new Error(`Lite 包禁止包含微信源码或运行时：${entryPath}`);
      }
      if (entry.isDirectory()) pending.push(entryPath);
      else if (entry.isFile()) {
        const content = readFileSync(entryPath).toString("utf8");
        if (forbiddenContent.some((pattern) => pattern.test(content))) {
          throw new Error(`Lite 包禁止包含微信源码标记：${entryPath}`);
        }
      }
    }
  }
}

console.log(`[macos-package] appVersion=${config.appVersion}`);
console.log(`[macos-package] releaseLabel=${config.releaseLabel}`);
console.log(`[macos-package] macosBundleVersion=${config.macosBundleVersion}`);

run("npm", ["run", "tauri", "--", "build", "--target", "universal-apple-darwin"]);
run("npm", ["run", "repair:macos-app", "--", config.macosAppPath, "--dmg", config.macosDmgPath]);
assertLiteResources(config.macosAppPath);
run("npm", ["run", "verify:macos-app", "--", config.macosAppPath]);
