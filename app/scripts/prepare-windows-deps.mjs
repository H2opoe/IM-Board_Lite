import { ensureRuntimeArchive } from "./runtime-download.mjs";
import { existsSync, mkdirSync, rmSync } from "node:fs";
import { cp, readdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const runtimeRoot = join(root, "runtime", "llama.cpp");
const version = "b8987";
const platform = {
  arch: "win-cpu-x64",
  archive: `llama-${version}-bin-win-cpu-x64.zip`,
};

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: "inherit",
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(`${command} ${args.join(" ")} failed`);
  }
}

async function ensureArchive(archivePath) {
  await ensureRuntimeArchive(archivePath, platform.archive);
}

async function ensureExtracted() {
  const platformRoot = join(runtimeRoot, version, platform.arch);
  const archivePath = join(platformRoot, platform.archive);
  const extractedDir = join(platformRoot, `llama-${version}`);
  const serverPath = join(extractedDir, "llama-server.exe");
  if (!existsSync(serverPath)) {
    await ensureArchive(archivePath);
    rmSync(extractedDir, { recursive: true, force: true });
    if (process.platform === "win32") {
      run("powershell.exe", [
        "-NoLogo",
        "-NoProfile",
        "-Command",
        `Expand-Archive -LiteralPath '${archivePath.replace(/'/g, "''")}' -DestinationPath '${platformRoot.replace(/'/g, "''")}' -Force`,
      ]);
    } else {
      run("unzip", ["-q", "-o", archivePath, "-d", platformRoot]);
    }
    if (!existsSync(serverPath) && existsSync(join(platformRoot, "llama-server.exe"))) {
      mkdirSync(extractedDir, { recursive: true });
      const entries = await readdir(platformRoot);
      for (const entry of entries) {
        if (entry === `llama-${version}` || entry === platform.archive) continue;
        await cp(join(platformRoot, entry), join(extractedDir, entry), {
          recursive: true,
          force: true,
        });
        rmSync(join(platformRoot, entry), { recursive: true, force: true });
      }
    }
  }
  rmSync(archivePath, { force: true });

  await readdir(extractedDir);
}

async function removeUnusedWindowsRuntimeFiles(path) {
  let entries;
  try {
    entries = await readdir(path, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    const child = join(path, entry.name);
    const lowerName = entry.name.toLowerCase();
    if (
      entry.name === ".DS_Store" ||
      lowerName === "cli-arm64.exe" ||
      lowerName === "gui-arm64.exe"
    ) {
      rmSync(child, { force: true });
    } else if (entry.isDirectory()) {
      await removeUnusedWindowsRuntimeFiles(child);
    }
  }
}

mkdirSync(runtimeRoot, { recursive: true });
await ensureExtracted();
for (const path of [runtimeRoot, join(root, "dist")]) {
  await removeUnusedWindowsRuntimeFiles(path);
}

console.log("[windows-deps] Lite Windows x64 llama.cpp runtime is ready");
