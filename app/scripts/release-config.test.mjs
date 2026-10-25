import { test } from "node:test";
import assert from "node:assert/strict";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");

function verifyFixture(platformConfig) {
  const root = mkdtempSync(join(tmpdir(), "imboard release metadata-"));
  try {
    mkdirSync(join(root, "scripts"));
    mkdirSync(join(root, "src-tauri"));
    for (const name of ["package.json", "package-lock.json", "release-manifest.json", "src-tauri/Cargo.toml", "src-tauri/tauri.conf.json", "scripts/release-config.mjs", "scripts/verify-release-config.mjs"]) {
      copyFileSync(join(appRoot, name), join(root, name));
    }
    const manifest = JSON.parse(readFileSync(join(root, "release-manifest.json"), "utf8"));
    manifest.target = "windows";
    writeFileSync(join(root, "release-manifest.json"), JSON.stringify(manifest));
    writeFileSync(join(root, "src-tauri/tauri.windows.conf.json"), JSON.stringify(platformConfig));
    const env = { ...process.env };
    for (const key of ["IM_BOARD_APP_VERSION", "IM_BOARD_RELEASE_LABEL", "IM_BOARD_MACOS_BUNDLE_VERSION"]) delete env[key];
    return spawnSync(process.execPath, [join(root, "scripts/verify-release-config.mjs")], { env, encoding: "utf8" });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("Windows build preserves the base resource list when its platform config changes other fields", () => {
  const result = verifyFixture({ build: { beforeBuildCommand: "npm run build" }, bundle: { icon: ["icons/icon-dark.ico"] } });
  assert.equal(result.status, 0, result.stderr);
});

test("Windows partial resource arrays cannot hide base runtime or notice additions", () => {
  const result = verifyFixture({ build: { beforeBuildCommand: "npm run build" }, bundle: { resources: ["icons/icon-dark.png"] } });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /平台配置不能覆盖资源清单/);
});

test("Windows resource-map overrides also fail the single-list contract", () => {
  const result = verifyFixture({ build: { beforeBuildCommand: "npm run build" }, bundle: { resources: { "icons/icon-dark.png": "icon.png" } } });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /平台配置不能覆盖资源清单/);
});

test("Windows native build hooks cannot bypass runtime and static license gates", () => {
  const result = verifyFixture({ build: { beforeBuildCommand: "tsc && vite build" }, bundle: {} });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /beforeBuildCommand 必须执行 npm run build/);
});
