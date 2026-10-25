import { ensureRuntimeArchive } from "./runtime-download.mjs";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { readdir, stat } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const runtimeRoot = join(root, "runtime", "llama.cpp");
const version = "b8987";
const platforms = [
  {
    arch: "darwin-arm64",
    archive: `llama-${version}-bin-macos-arm64.tar.gz`,
  },
  {
    arch: "darwin-x64",
    archive: `llama-${version}-bin-macos-x64.tar.gz`,
  },
];

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: "inherit",
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed`);
  }
}

async function ensureArchive(platform, archivePath) {
  await ensureRuntimeArchive(archivePath, platform.archive);
}

async function ensureExtracted(platform) {
  const platformRoot = join(runtimeRoot, version, platform.arch);
  const archivePath = join(platformRoot, platform.archive);
  const extractedDir = join(platformRoot, `llama-${version}`);
  const serverPath = join(extractedDir, "llama-server");
  if (!existsSync(serverPath)) {
    await ensureArchive(platform, archivePath);
    rmSync(extractedDir, { recursive: true, force: true });
    try {
      run("tar", ["-xzf", archivePath, "-C", platformRoot]);
    } catch (error) {
      console.warn(`[macos-deps] ${platform.archive} is not extractable, downloading it again`);
      rmSync(archivePath, { force: true });
      rmSync(extractedDir, { recursive: true, force: true });
      await ensureArchive(platform, archivePath);
      run("tar", ["-xzf", archivePath, "-C", platformRoot]);
    }
  }
  rmSync(archivePath, { force: true });
  const entries = await readdir(extractedDir);
  for (const entry of entries) {
    const fullPath = join(extractedDir, entry);
    const info = await stat(fullPath);
    if (info.isFile() && (entry.startsWith("llama-") || entry === "rpc-server")) {
      run("chmod", ["755", fullPath]);
    }
  }
}

mkdirSync(runtimeRoot, { recursive: true });
for (const platform of platforms) {
  await ensureExtracted(platform);
}

console.log("[macos-deps] llama.cpp macOS arm64/x64 runtimes are ready");
