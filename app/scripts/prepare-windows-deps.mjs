import { createWriteStream, existsSync, mkdirSync, rmSync } from "node:fs";
import { cp, readdir } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { pipeline } from "node:stream/promises";
import { Readable } from "node:stream";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const runtimeRoot = join(root, "runtime", "llama.cpp");
const pythonRuntimeRoot = join(root, "runtime", "python");
const version = "b8987";
const pythonStandaloneRelease = "20260414";
const pythonVersion = "3.10.20";
const platform = {
  arch: "win-cpu-x64",
  archive: `llama-${version}-bin-win-cpu-x64.zip`,
};
const pythonPlatform = {
  arch: "win-x64",
  target: "x86_64-pc-windows-msvc",
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

async function download(url, to) {
  console.log(`[windows-deps] downloading ${url}`);
  const response = await fetch(url);
  if (!response.ok || !response.body) {
    throw new Error(`${url} returned ${response.status}`);
  }
  mkdirSync(dirname(to), { recursive: true });
  await pipeline(Readable.fromWeb(response.body), createWriteStream(to));
}

async function ensureArchive(archivePath) {
  if (existsSync(archivePath)) return;
  const officialUrl = `https://github.com/ggml-org/llama.cpp/releases/download/${version}/${platform.archive}`;
  const mirrors = [`https://gh-proxy.com/${officialUrl}`, `https://gh.llkk.cc/${officialUrl}`, officialUrl];
  const errors = [];
  for (const url of mirrors) {
    try {
      await download(url, archivePath);
      return;
    } catch (error) {
      errors.push(`${url}: ${error.message}`);
      if (existsSync(archivePath)) rmSync(archivePath, { force: true });
    }
  }
  throw new Error(`failed to download ${platform.archive}: ${errors.join("; ")}`);
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

function pythonArchiveName() {
  return `cpython-${pythonVersion}+${pythonStandaloneRelease}-${pythonPlatform.target}-install_only_stripped.tar.gz`;
}

async function ensurePythonRuntime() {
  const platformRoot = join(pythonRuntimeRoot, pythonStandaloneRelease, pythonPlatform.arch);
  const archivePath = join(platformRoot, pythonArchiveName());
  const extractedDir = join(platformRoot, "python");
  const pythonPath = join(extractedDir, "python.exe");
  if (!existsSync(pythonPath)) {
    await ensurePythonArchive(archivePath);
    rmSync(extractedDir, { recursive: true, force: true });
    run("tar", ["-xzf", archivePath, "-C", platformRoot]);
  }
  rmSync(archivePath, { force: true });
  await readdir(extractedDir);
  ensureWechatPythonDependencies(pythonPath);
}

async function ensurePythonArchive(archivePath) {
  if (existsSync(archivePath)) return;
  const archive = pythonArchiveName();
  const officialUrl = `https://github.com/astral-sh/python-build-standalone/releases/download/${pythonStandaloneRelease}/${archive}`;
  const mirrors = [`https://gh-proxy.com/${officialUrl}`, `https://gh.llkk.cc/${officialUrl}`, officialUrl];
  const errors = [];
  for (const url of mirrors) {
    try {
      await download(url, archivePath);
      return;
    } catch (error) {
      errors.push(`${url}: ${error.message}`);
      if (existsSync(archivePath)) rmSync(archivePath, { force: true });
    }
  }
  throw new Error(`failed to download ${archive}: ${errors.join("; ")}`);
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
      lowerName === "pymacconfig.h" ||
      lowerName === "macos.py" ||
      lowerName === "macosx.py" ||
      lowerName === "_macos.py" ||
      lowerName === "_macos_compat.py" ||
      lowerName === "_osx_support.py" ||
      lowerName === "macosx_libfile.py" ||
      lowerName === "cli-arm64.exe" ||
      lowerName === "gui-arm64.exe"
    ) {
      rmSync(child, { force: true });
    } else if (entry.isDirectory()) {
      await removeUnusedWindowsRuntimeFiles(child);
    }
  }
}

function ensureWechatPythonDependencies(pythonPath) {
  const sitePackages = join(dirname(pythonPath), "Lib", "site-packages");
  const requiredMarkers = [
    join(sitePackages, "click"),
    join(sitePackages, "Crypto"),
    join(sitePackages, "zstandard"),
  ];
  if (requiredMarkers.every(existsSync)) return;
  console.log("[windows-deps] installing bundled WeChat Python dependencies");
  run(pythonPath, [
    "-m",
    "pip",
    "install",
    "--disable-pip-version-check",
    "--no-cache-dir",
    "--only-binary=:all:",
    "--target",
    sitePackages,
    "click>=8.1,<9",
    "pycryptodome>=3.19,<4",
    "zstandard>=0.22,<1",
  ]);
}

mkdirSync(runtimeRoot, { recursive: true });
await ensureExtracted();
mkdirSync(pythonRuntimeRoot, { recursive: true });
await ensurePythonRuntime();
for (const path of [runtimeRoot, pythonRuntimeRoot, join(root, "dist")]) {
  await removeUnusedWindowsRuntimeFiles(path);
}

console.log("[windows-deps] llama.cpp and Python Windows x64 runtimes are ready");
