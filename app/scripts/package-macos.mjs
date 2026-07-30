import { readdirSync } from "node:fs";
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
  const forbidden = /(^|[\\/])(wechat-cli-source|wechat_cli|wechat_bridge|bridge_main|wx_key|python)([\\/._-]|$)/i;
  const pending = [join(appPath, "Contents")];
  while (pending.length > 0) {
    const current = pending.pop();
    for (const entry of readdirSync(current, { withFileTypes: true })) {
      const entryPath = join(current, entry.name);
      if (forbidden.test(entryPath)) {
        throw new Error(`Lite 包禁止包含微信源码或运行时：${entryPath}`);
      }
      if (entry.isDirectory()) pending.push(entryPath);
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
