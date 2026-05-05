import { createWriteStream, existsSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { readdir, stat } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { pipeline } from "node:stream/promises";
import { Readable } from "node:stream";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const runtimeRoot = join(root, "runtime", "llama.cpp");
const pythonRuntimeRoot = join(root, "runtime", "python");
const wechatCliSourceRoot = join(root, "third_party", "wechat_bridge", "wechat-cli-source");
const version = "b8987";
const pythonStandaloneRelease = "20260414";
const pythonVersion = "3.10.20";
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
const pythonPlatforms = [
  {
    arch: "darwin-arm64",
    target: "aarch64-apple-darwin",
  },
  {
    arch: "darwin-x64",
    target: "x86_64-apple-darwin",
  },
];
const wechatKeyScannerPlatforms = [
  {
    arch: "arm64",
    output: "find_all_keys_macos.arm64",
  },
  {
    arch: "x86_64",
    output: "find_all_keys_macos.x86_64",
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

function commandSucceeds(command, args, options = {}) {
  const result = spawnSync(command, args, {
    stdio: "pipe",
    ...options,
  });
  return result.status === 0;
}

async function download(url, to) {
  console.log(`[macos-deps] downloading ${url}`);
  const response = await fetch(url);
  if (!response.ok || !response.body) {
    throw new Error(`${url} returned ${response.status}`);
  }
  mkdirSync(dirname(to), { recursive: true });
  await pipeline(Readable.fromWeb(response.body), createWriteStream(to));
}

async function ensureArchive(platform, archivePath) {
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

async function ensurePythonArchive(platform, archivePath) {
  if (existsSync(archivePath)) return;
  const archive = pythonArchiveName(platform);
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

function pythonArchiveName(platform) {
  return `cpython-${pythonVersion}+${pythonStandaloneRelease}-${platform.target}-install_only_stripped.tar.gz`;
}

async function ensurePythonExtracted(platform) {
  const platformRoot = join(pythonRuntimeRoot, pythonStandaloneRelease, platform.arch);
  const archivePath = join(platformRoot, pythonArchiveName(platform));
  const extractedDir = join(platformRoot, "python");
  const pythonPath = join(extractedDir, "bin", "python3.10");
  const shouldExtract = !existsSync(pythonPath) || !isPythonRuntimeUsable(pythonPath);
  if (shouldExtract) {
    if (existsSync(pythonPath)) {
      console.warn(`[macos-deps] ${platform.arch} Python runtime is incomplete, extracting a clean copy`);
    }
    await ensurePythonArchive(platform, archivePath);
    rmSync(extractedDir, { recursive: true, force: true });
    try {
      run("tar", ["-xzf", archivePath, "-C", platformRoot]);
    } catch (error) {
      const archive = pythonArchiveName(platform);
      console.warn(`[macos-deps] ${archive} is not extractable, downloading it again`);
      rmSync(archivePath, { force: true });
      rmSync(extractedDir, { recursive: true, force: true });
      await ensurePythonArchive(platform, archivePath);
      run("tar", ["-xzf", archivePath, "-C", platformRoot]);
    }
  }
  rmSync(archivePath, { force: true });
  run("chmod", ["755", pythonPath]);
}

function isPythonRuntimeUsable(pythonPath) {
  // 半截运行时会留下 python3.10，但 pip vendor 或 site-packages 可能缺文件，必须在打包前主动发现。
  return commandSucceeds(pythonPath, ["-c", "import pip._vendor.platformdirs.macos"]);
}

async function removeWindowsLaunchersFromPython(platform) {
  const pythonRoot = join(pythonRuntimeRoot, pythonStandaloneRelease, platform.arch, "python");
  await removeFilesMatching(pythonRoot, (entry) => entry.toLowerCase().endsWith(".exe"));
}

async function removeWindowsOnlyPythonFiles(platform) {
  const pythonRoot = join(pythonRuntimeRoot, pythonStandaloneRelease, platform.arch, "python");
  const windowsOnlyPatterns = [
    /(^|_)windows(_|\.)/i,
    /(^|_)win32(_|\.)/i,
    /^wintypes\.py$/i,
    /^cygwinccompiler\.py$/i,
    /^popen_spawn_win32\.py$/i,
    /^_winconsole\.py$/i,
    /^scanner_windows\.py$/i,
  ];
  await removeFilesMatching(pythonRoot, (entry) => {
    return windowsOnlyPatterns.some((pattern) => pattern.test(entry));
  });
}

async function removeFilesMatching(rootPath, shouldRemove) {
  if (!existsSync(rootPath)) return;
  const entries = await readdir(rootPath);
  for (const entry of entries) {
    const fullPath = join(rootPath, entry);
    const info = await stat(fullPath);
    if (info.isDirectory()) {
      await removeFilesMatching(fullPath, shouldRemove);
      continue;
    }
    if (info.isFile() && shouldRemove(entry)) {
      rmSync(fullPath, { force: true });
    }
  }
}

function cleanWechatCliBuildArtifacts() {
  for (const name of ["build", "dist"]) {
    rmSync(join(wechatCliSourceRoot, name), { recursive: true, force: true });
  }
}

function cleanInstalledWechatCli(platform) {
  const sitePackages = join(
    pythonRuntimeRoot,
    pythonStandaloneRelease,
    platform.arch,
    "python",
    "lib",
    "python3.10",
    "site-packages",
  );
  for (const name of ["wechat_cli", "wechat_cli.egg-info", "build", "image", "npm"]) {
    rmSync(join(sitePackages, name), { recursive: true, force: true });
  }
  if (!existsSync(sitePackages)) return;
  for (const name of readdirSync(sitePackages)) {
    if (/^wechat_cli-.*\.dist-info$/.test(name)) {
      rmSync(join(sitePackages, name), { recursive: true, force: true });
    }
  }
}

function ensureWechatKeyScannerBinaries() {
  const sourcePath = join(wechatCliSourceRoot, "wechat_cli", "bin", "find_all_keys_macos.c");
  if (!existsSync(sourcePath)) {
    throw new Error(`[macos-deps] missing WeChat key scanner source: ${sourcePath}`);
  }
  for (const platform of wechatKeyScannerPlatforms) {
    const outputPath = join(wechatCliSourceRoot, "wechat_cli", "bin", platform.output);
    run("cc", [
      "-O2",
      "-arch",
      platform.arch,
      "-o",
      outputPath,
      sourcePath,
      "-framework",
      "Foundation",
    ]);
    run("chmod", ["755", outputPath]);
  }
}

function installWechatCliIntoPython(platform) {
  if (!existsSync(wechatCliSourceRoot)) {
    throw new Error(`[macos-deps] missing vendored wechat-cli source: ${wechatCliSourceRoot}`);
  }
  const pythonPath = join(
    pythonRuntimeRoot,
    pythonStandaloneRelease,
    platform.arch,
    "python",
    "bin",
    "python3.10",
  );
  run(pythonPath, [
    "-m",
    "pip",
    "install",
    "--disable-pip-version-check",
    "--no-compile",
    "--force-reinstall",
    wechatCliSourceRoot,
  ]);
  const cliPath = join(
    pythonRuntimeRoot,
    pythonStandaloneRelease,
    platform.arch,
    "python",
    "bin",
    "wechat-cli",
  );
  rewriteWechatCliEntrypoint(cliPath);
  run("chmod", ["755", cliPath]);
  run(pythonPath, ["-c", "import wechat_cli, wechat_cli.main"]);
}

function rewriteWechatCliEntrypoint(cliPath) {
  writeFileSync(
    cliPath,
    `#!/bin/sh
'''exec' "$(dirname "$0")/python3.10" "$0" "$@"
' '''
import sys
from wechat_cli.main import cli

if __name__ == '__main__':
    sys.argv[0] = sys.argv[0].removesuffix('.exe')
    sys.exit(cli())
`,
  );
}

async function removeFinderMetadata(path) {
  let entries;
  try {
    entries = await readdir(path, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    const child = join(path, entry.name);
    if (entry.name === ".DS_Store") {
      rmSync(child, { force: true });
    } else if (entry.isDirectory()) {
      await removeFinderMetadata(child);
    }
  }
}

mkdirSync(runtimeRoot, { recursive: true });
for (const platform of platforms) {
  await ensureExtracted(platform);
}
mkdirSync(pythonRuntimeRoot, { recursive: true });
cleanWechatCliBuildArtifacts();
ensureWechatKeyScannerBinaries();
for (const platform of pythonPlatforms) {
  cleanWechatCliBuildArtifacts();
  await ensurePythonExtracted(platform);
  cleanInstalledWechatCli(platform);
  installWechatCliIntoPython(platform);
  await removeWindowsLaunchersFromPython(platform);
  await removeWindowsOnlyPythonFiles(platform);
}
for (const path of [runtimeRoot, pythonRuntimeRoot, join(root, "bridges"), join(root, "third_party"), join(root, "dist")]) {
  await removeFinderMetadata(path);
}

console.log("[macos-deps] llama.cpp, Python, and WeChat CLI macOS arm64/x64 runtimes are ready");
