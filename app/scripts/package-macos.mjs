import { spawnSync } from "node:child_process";
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

console.log(`[macos-package] appVersion=${config.appVersion}`);
console.log(`[macos-package] releaseLabel=${config.releaseLabel}`);
console.log(`[macos-package] macosBundleVersion=${config.macosBundleVersion}`);

run("npm", ["run", "tauri", "--", "build", "--target", "universal-apple-darwin"]);
run("npm", ["run", "repair:macos-app", "--", config.macosAppPath, "--dmg", config.macosDmgPath]);
run("npm", ["run", "verify:macos-app", "--", config.macosAppPath]);
