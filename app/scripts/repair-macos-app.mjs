import { resolve } from "node:path";
import { releaseConfig } from "./release-config.mjs";
import { createVerifiedDmg, repairMacosBundle } from "./macos-package-artifacts.mjs";

const config = releaseConfig();
const args = process.argv.slice(2);
const appPath = resolve(args[0] && !args[0].startsWith("--") ? args[0] : config.macosAppPath);
const dmgIndex = args.indexOf("--dmg");
if (dmgIndex >= 0 && (!args[dmgIndex + 1] || args[dmgIndex + 1].startsWith("--"))) throw new Error("--dmg 需要输出路径。");
const dmgPath = dmgIndex >= 0 ? resolve(args[dmgIndex + 1]) : null;
const signingIdentity = process.env.IM_BOARD_MACOS_SIGNING_IDENTITY ?? process.env.APPLE_SIGNING_IDENTITY ?? "-";
repairMacosBundle({ appPath, config, signingIdentity });
if (dmgPath) createVerifiedDmg({ appPath, dmgPath, productName: config.productName });
console.log(`[macos-app] 已校验并替换 App：${appPath}`);
if (dmgPath) console.log(`[macos-app] 已校验并生成 DMG：${dmgPath}`);
