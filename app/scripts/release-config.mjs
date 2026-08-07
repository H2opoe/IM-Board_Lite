import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const releaseManifest = JSON.parse(readFileSync(join(appRoot, "release-manifest.json"), "utf8"));

function requireStableAppVersion(value) {
  if (!/^\d+\.\d+\.\d+$/.test(value)) {
    throw new Error(`package.json version 必须是不带 beta 后缀的三段系统版本号，当前为 ${value}`);
  }
}

function requireReleaseLabel(value) {
  if (!/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(value)) {
    throw new Error(`release.label 必须是合法发布标签，当前为 ${value}`);
  }
}

function defaultMacosBundleVersion(appVersion, releaseLabel) {
  const [major, minor, patch] = appVersion.split(".").map(Number);
  const betaMatch = releaseLabel.match(/(?:^|[-.])beta\.?(\d+)$/i);
  const build = betaMatch ? Number(betaMatch[1]) : 0;
  return `${major}${String(minor).padStart(2, "0")}${String(patch).padStart(2, "0")}${String(build).padStart(2, "0")}`;
}

export function releaseConfig() {
  const appVersion = process.env.IM_BOARD_APP_VERSION || releaseManifest.appVersion;
  const releaseLabel = process.env.IM_BOARD_RELEASE_LABEL || releaseManifest.releaseLabel;
  const macosBundleVersion =
    process.env.IM_BOARD_MACOS_BUNDLE_VERSION ||
    releaseManifest.macosBundleVersion ||
    defaultMacosBundleVersion(appVersion, releaseLabel);

  requireStableAppVersion(appVersion);
  requireReleaseLabel(releaseLabel);
  if (!/^\d+$/.test(macosBundleVersion)) {
    throw new Error(`macOS CFBundleVersion 必须是递增数字构建号，当前为 ${macosBundleVersion}`);
  }

  return {
    appRoot,
    appVersion,
    releaseLabel,
    macosBundleVersion,
    edition: releaseManifest.edition,
    target: releaseManifest.target,
    bundleId: "com.local.im-board",
    productName: "IM-Board",
    macosAppPath: join(appRoot, "src-tauri", "target", "universal-apple-darwin", "release", "bundle", "macos", "IM-Board.app"),
    macosDmgPath: join(appRoot, "..", "artifacts", "macos", `IM-Board_${releaseLabel}_mac_universal.dmg`),
  };
}
