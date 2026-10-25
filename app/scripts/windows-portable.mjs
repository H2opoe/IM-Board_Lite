import { closeSync, cpSync, existsSync, lstatSync, mkdirSync, mkdtempSync, openSync, readSync, readdirSync, renameSync, rmSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { createHash } from "node:crypto";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import * as zlib from "node:zlib";

export function requireFile(file, label) {
  if (!existsSync(file) || !lstatSync(file).isFile()) throw new Error(`${label} 不存在或不是普通文件：${file}`);
}

export function resolveTargetArtifacts(appRoot, binaryNames, env = process.env) {
  const cargoRoot = join(appRoot, "src-tauri");
  const targetRoot = env.CARGO_TARGET_DIR ? resolve(cargoRoot, env.CARGO_TARGET_DIR) : join(cargoRoot, "target");
  const release = join(targetRoot, "x86_64-pc-windows-msvc", "release");
  const binary = binaryNames.map(name => join(release, name)).find(file => existsSync(file) && lstatSync(file).isFile());
  if (!binary) throw new Error(`Windows 主程序不存在：${binaryNames.map(name => join(release, name)).join(" / ")}`);
  const library = [join(release, "im_board_lib.dll"), join(release, "deps", "im_board_lib.dll")].find(file => existsSync(file) && lstatSync(file).isFile());
  if (!library) throw new Error(`Windows Rust 动态库不存在：${release}`);
  const resources = join(release, "_up_");
  if (!existsSync(resources) || !lstatSync(resources).isDirectory()) throw new Error(`Windows 打包资源目录不存在：${resources}`);
  return { release, binary, library, resources };
}

function checkName(name) {
  if (!name || /[\x00-\x1f\x7f\\:<>"|?*]/.test(name) || name.startsWith("/") || name.includes("//")) throw new Error(`归档路径无效：${name}`);
  const trimmed = name.endsWith("/") ? name.slice(0, -1) : name;
  if (trimmed.split("/").some(part => !part || part === "." || part === ".." || /[ .]$/.test(part) || /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\.|$)/i.test(part))) throw new Error(`归档路径不能安全用于 Windows：${name}`);
  return trimmed;
}

let crcTable;
function crc32(bytes, previous = 0) {
  if (zlib.crc32) return zlib.crc32(bytes, previous);
  crcTable ??= Array.from({ length: 256 }, (_, value) => { for (let i = 0; i < 8; i++) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1; return value >>> 0; });
  let crc = previous ^ 0xffffffff;
  for (const byte of bytes) crc = crcTable[(crc ^ byte) & 255] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

function fileIdentity(file) {
  const fd = openSync(file, "r"), sha = createHash("sha256"), buffer = Buffer.alloc(128 * 1024);
  let bytes = 0, crc = 0;
  try {
    for (let n; (n = readSync(fd, buffer, 0, buffer.length, null)) > 0;) { const chunk = buffer.subarray(0, n); sha.update(chunk); crc = crc32(chunk, crc); bytes += n; }
  } finally { closeSync(fd); }
  return { bytes, sha256: sha.digest("hex"), crc32: crc };
}

export function inspectTree(root, { cleanMetadata = false } = {}) {
  if (!lstatSync(root).isDirectory()) throw new Error(`打包目录无效：${root}`);
  const files = Object.create(null), directories = [], seen = new Set();
  function visit(directory, prefix = "") {
    for (const name of readdirSync(directory).sort()) {
      const relative = prefix ? `${prefix}/${name}` : name, file = join(directory, name), stat = lstatSync(file);
      checkName(relative);
      const key = relative.toLowerCase();
      if (seen.has(key)) throw new Error(`Windows 大小写路径冲突：${relative}`); seen.add(key);
      if (stat.isSymbolicLink() || (!stat.isFile() && !stat.isDirectory())) throw new Error(`打包输入禁止链接或特殊文件：${relative}`);
      if (cleanMetadata && (name === ".DS_Store" || name === "__pycache__")) { rmSync(file, { recursive: true, force: true }); continue; }
      if (stat.isDirectory()) { directories.push(relative); visit(file, relative); }
      else files[relative] = fileIdentity(file);
    }
  }
  visit(root);
  return { files, directories: directories.sort() };
}

function safeNumber(value) {
  const number = Number(value);
  if (!Number.isSafeInteger(number) || number < 0) throw new Error("ZIP64 数值超出安全范围");
  return number;
}

function readExtraFields(extra, filename) {
  const fields = new Map();
  for (let cursor = 0; cursor < extra.length;) {
    if (cursor + 4 > extra.length) throw new Error("ZIP 扩展字段截断");
    const tag = extra.readUInt16LE(cursor), length = extra.readUInt16LE(cursor + 2); cursor += 4;
    if (cursor + length > extra.length) throw new Error("ZIP 扩展字段截断");
    const value = extra.subarray(cursor, cursor + length); cursor += length;
    if (fields.has(tag)) throw new Error("ZIP 扩展字段重复"); fields.set(tag, value);
    if (tag === 0x7075) {
      if (value.length < 5 || value[0] !== 1 || value.readUInt32LE(1) !== crc32(filename)) throw new Error("ZIP Unicode 路径版本或文件名 CRC 无效");
      const unicode = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(value.subarray(5));
      if (!Buffer.from(unicode, "utf8").equals(filename)) throw new Error("ZIP Unicode 路径与原文件名不一致");
    }
  }
  return fields;
}

// Read only the central directory; filenames and entry kinds are checked before
// any extraction tool can create a file or follow a link.
export function readArchiveEntries(archive) {
  const size = statSync(archive).size, fd = openSync(archive, "r");
  function read(position, length) {
    if (!Number.isSafeInteger(position) || position < 0 || length < 0 || position + length > size) throw new Error("ZIP 目录越界");
    const buffer = Buffer.alloc(length);
    if (readSync(fd, buffer, 0, length, position) !== length) throw new Error("ZIP 目录截断");
    return buffer;
  }
  try {
    const tailLength = Math.min(size, 65557), tail = read(size - tailLength, tailLength);
    let end = -1;
    for (let i = tail.length - 22; i >= 0; i--) if (tail.readUInt32LE(i) === 0x06054b50 && i + 22 + tail.readUInt16LE(i + 20) === tail.length) { end = i; break; }
    if (end < 0) throw new Error("ZIP 缺少有效结束目录");
    if (tail.readUInt16LE(end + 4) !== 0 || tail.readUInt16LE(end + 6) !== 0) throw new Error("不支持分卷 ZIP");
    let count = tail.readUInt16LE(end + 10), directorySize = tail.readUInt32LE(end + 12), position = tail.readUInt32LE(end + 16);
    if (count === 0xffff || directorySize === 0xffffffff || position === 0xffffffff) {
      const locator = read(size - tailLength + end - 20, 20);
      if (locator.readUInt32LE(0) !== 0x07064b50 || locator.readUInt32LE(4) !== 0 || locator.readUInt32LE(16) !== 1) throw new Error("ZIP64 定位无效");
      const header = read(safeNumber(locator.readBigUInt64LE(8)), 56);
      if (header.readUInt32LE(0) !== 0x06064b50 || header.readBigUInt64LE(4) < 44n || header.readUInt32LE(16) !== 0 || header.readUInt32LE(20) !== 0 || header.readBigUInt64LE(24) !== header.readBigUInt64LE(32)) throw new Error("ZIP64 目录无效");
      count = safeNumber(header.readBigUInt64LE(32)); directorySize = safeNumber(header.readBigUInt64LE(40)); position = safeNumber(header.readBigUInt64LE(48));
    } else if (tail.readUInt16LE(end + 8) !== count) throw new Error("ZIP 条目数量不一致");
    const directoryEnd = position + directorySize;
    if (directoryEnd > size - tailLength + end || count > directorySize / 46) throw new Error("ZIP 中央目录无效");
    const entries = [], seen = new Set();
    for (let index = 0; index < count; index++) {
      const header = read(position, 46);
      if (header.readUInt32LE(0) !== 0x02014b50 || (header.readUInt16LE(8) & 1)) throw new Error("ZIP 条目无效或已加密");
      const nameLength = header.readUInt16LE(28), extraLength = header.readUInt16LE(30), commentLength = header.readUInt16LE(32);
      const filename = read(position + 46, nameLength);
      const name = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(filename);
      const extraFields = readExtraFields(read(position + 46 + nameLength, extraLength), filename);
      const canonical = checkName(name), key = canonical.toLowerCase();
      if (seen.has(key)) throw new Error(`ZIP 重复或大小写冲突条目：${name}`); seen.add(key);
      const mode = header.readUInt32LE(38) >>> 16, kind = mode & 0xf000;
      if (kind && kind !== 0x8000 && kind !== 0x4000) throw new Error(`ZIP 禁止链接或特殊条目：${name}`);
      if ((kind === 0x4000 && !name.endsWith("/")) || (kind === 0x8000 && name.endsWith("/"))) throw new Error(`ZIP 条目类型不一致：${name}`);
      let bytes = header.readUInt32LE(24), compressed = header.readUInt32LE(20), localOffset = header.readUInt32LE(42), disk = header.readUInt16LE(34);
      if (bytes === 0xffffffff || compressed === 0xffffffff || localOffset === 0xffffffff || disk === 0xffff) {
        const values = extraFields.get(1);
        if (!values) throw new Error("ZIP64 条目缺少扩展字段"); let cursor = 0;
        const next = () => { if (cursor + 8 > values.length) throw new Error("ZIP64 条目截断"); const n = safeNumber(values.readBigUInt64LE(cursor)); cursor += 8; return n; };
        if (bytes === 0xffffffff) bytes = next(); if (compressed === 0xffffffff) compressed = next(); if (localOffset === 0xffffffff) localOffset = next();
        if (disk === 0xffff) { if (cursor + 4 > values.length) throw new Error("ZIP64 磁盘字段截断"); disk = values.readUInt32LE(cursor); }
      }
      if (disk !== 0 || localOffset >= position || localOffset + 30 + compressed > size) throw new Error("ZIP 本地条目无效");
      const local = read(localOffset, 30);
      if (local.readUInt32LE(0) !== 0x04034b50 || local.readUInt16LE(6) !== header.readUInt16LE(8) || local.readUInt16LE(8) !== header.readUInt16LE(10)) throw new Error("ZIP 本地条目与中央目录不一致");
      const localName = read(localOffset + 30, local.readUInt16LE(26));
      if (!localName.equals(filename) || localOffset + 30 + local.readUInt16LE(26) + local.readUInt16LE(28) + compressed > directoryEnd - directorySize) throw new Error("ZIP 本地文件名或数据范围不一致");
      readExtraFields(read(localOffset + 30 + local.readUInt16LE(26), local.readUInt16LE(28)), localName);
      if (name.endsWith("/") && (bytes !== 0 || header.readUInt32LE(16) !== 0)) throw new Error(`ZIP 目录条目含文件数据：${name}`);
      entries.push({ name, canonical, directory: name.endsWith("/"), bytes, crc32: header.readUInt32LE(16) });
      position += 46 + nameLength + extraLength + commentLength;
      if (position > directoryEnd) throw new Error("ZIP 中央目录截断");
    }
    if (position !== directoryEnd) throw new Error("ZIP 中央目录长度不一致");
    return entries;
  } finally { closeSync(fd); }
}

function requiredRun(run, command, args, options) {
  const result = run(command, args, { encoding: "utf8", maxBuffer: 1024 * 1024, ...options });
  if (result.error || result.status !== 0) throw new Error(`${command} 失败：${result.error?.message || result.stderr?.trim() || `退出码 ${result.status}`}`);
  return result;
}

const psCompress = String.raw`Add-Type -AssemblyName System.IO.Compression; Add-Type -AssemblyName System.IO.Compression.FileSystem
$root = [IO.Path]::GetFullPath($env:IM_BOARD_PORTABLE_STAGE).TrimEnd([char]92, [char]47)
$top = [IO.Path]::GetFileName($root)
$stream = [IO.File]::Open($env:IM_BOARD_PORTABLE_ARCHIVE, [IO.FileMode]::CreateNew)
$archive = $null
try {
  $archive = [IO.Compression.ZipArchive]::new($stream, [IO.Compression.ZipArchiveMode]::Create, $false)
  $null = $archive.CreateEntry($top + '/')
  foreach ($item in Get-ChildItem -LiteralPath $root -Force -Recurse) {
    $relative = $item.FullName.Substring($root.Length + 1).Replace([char]92, [char]47)
    $name = $top + '/' + $relative
    if ($item.PSIsContainer) { $null = $archive.CreateEntry($name + '/'); continue }
    $entry = $archive.CreateEntry($name, [IO.Compression.CompressionLevel]::Optimal)
    $input = [IO.File]::OpenRead($item.FullName)
    $output = $null
    try { $output = $entry.Open(); $input.CopyTo($output) }
    finally { if ($null -ne $output) { $output.Dispose() }; $input.Dispose() }
  }
} finally { if ($null -ne $archive) { $archive.Dispose() }; $stream.Dispose() }`;
const psExtract = "Add-Type -AssemblyName System.IO.Compression.FileSystem; [IO.Compression.ZipFile]::ExtractToDirectory($env:IM_BOARD_PORTABLE_ARCHIVE, $env:IM_BOARD_PORTABLE_VERIFY)";
function powershell(run, script, env) {
  requiredRun(run, "powershell.exe", ["-NoLogo", "-NoProfile", "-NonInteractive", "-EncodedCommand", Buffer.from(`$ErrorActionPreference = 'Stop'; ${script}`, "utf16le").toString("base64")], { env });
}

export function createPortableArchive(stage, archive, verifyRoot, { run = spawnSync, platform = process.platform, env = process.env } = {}) {
  const manifest = inspectTree(stage), top = checkName(stage.split(/[\\/]/).at(-1));
  const compression = run("zip", ["-X", "-q", "-r", archive, top], { cwd: dirname(stage), env, encoding: "utf8" });
  const psEnv = { ...env, IM_BOARD_PORTABLE_STAGE: stage, IM_BOARD_PORTABLE_ARCHIVE: archive, IM_BOARD_PORTABLE_VERIFY: verifyRoot };
  if (compression.error || compression.status !== 0) {
    if (platform !== "win32") throw new Error(`zip 打包失败：${compression.error?.message || compression.stderr?.trim() || compression.status}`);
    rmSync(archive, { force: true }); powershell(run, psCompress, psEnv);
  }
  requireFile(archive, "候选归档");
  const entries = readArchiveEntries(archive), actualFiles = new Set();
  const dirs = new Set([top, ...manifest.directories.map(p => `${top}/${p}`)]);
  for (const entry of entries) {
    if (entry.directory) { if (!dirs.has(entry.canonical)) throw new Error(`ZIP 包含多余目录：${entry.name}`); continue; }
    if (!entry.name.startsWith(`${top}/`)) throw new Error(`ZIP 顶层目录不一致：${entry.name}`);
    const relative = entry.name.slice(top.length + 1), expected = manifest.files[relative];
    if (!expected || expected.bytes !== entry.bytes || expected.crc32 !== entry.crc32) throw new Error(`ZIP 文件清单或完整性不一致：${entry.name}`);
    actualFiles.add(relative);
  }
  if (actualFiles.size !== Object.keys(manifest.files).length) throw new Error("ZIP 缺少暂存文件");
  mkdirSync(verifyRoot, { recursive: false });
  if (platform === "win32") powershell(run, psExtract, psEnv);
  else {
    requiredRun(run, "unzip", ["-t", archive], { env });
    requiredRun(run, "unzip", ["-q", archive, "-d", verifyRoot], { env });
  }
  const verified = inspectTree(join(verifyRoot, top));
  if (JSON.stringify(manifest) !== JSON.stringify(verified)) {
    const differentFiles = [...new Set([...Object.keys(manifest.files), ...Object.keys(verified.files)])]
      .filter(path => JSON.stringify(manifest.files[path]) !== JSON.stringify(verified.files[path]));
    const differentDirectories = [...new Set([...manifest.directories, ...verified.directories])]
      .filter(path => manifest.directories.includes(path) !== verified.directories.includes(path));
    throw new Error(`ZIP 解压文件 SHA-256 或目录不一致：${JSON.stringify({ files: differentFiles.slice(0, 10), directories: differentDirectories.slice(0, 10) })}`);
  }
  return { manifest, verifiedStage: join(verifyRoot, top) };
}

export function packagePortable(config, { run = spawnSync, platform = process.platform, env = process.env, log = console.log } = {}) {
  const { appRoot, productName, releaseLabel, templateVariant, freshTemplate = false, freshResources = false, executableName, binaryNames, prepareStage = () => {}, validateStage = () => {}, removeLegacyExecutables = [] } = config;
  checkName(productName); checkName(executableName);
  if (productName.includes("/") || executableName.includes("/") || !/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(releaseLabel)) throw new Error("打包名称或发行版本无效");
  const artifacts = resolveTargetArtifacts(appRoot, binaryNames, env), repo = resolve(appRoot, "..");
  const template = join(repo, "artifacts", "tmp", "release-windows-portable", templateVariant, "IM-Board");
  if (!freshTemplate && (!existsSync(template) || !lstatSync(template).isDirectory())) throw new Error(`release portable 模板不存在：${template}`);
  if (!freshTemplate) inspectTree(template); if (!freshResources) inspectTree(artifacts.resources);
  const outputDir = join(repo, "artifacts", "windows"); mkdirSync(outputDir, { recursive: true });
  const output = join(outputDir, `IM-Board_${releaseLabel}_windows_x64_portable.zip`);
  const workspace = mkdtempSync(join(outputDir, ".portable-build-"));
  const scratch = mkdtempSync(join(tmpdir(), "imboard-portable-stage-"));
  try {
    const payload = join(scratch, "payload"), stage = join(payload, productName); mkdirSync(payload);
    if (freshTemplate) mkdirSync(stage); else cpSync(template, stage, { recursive: true });
    rmSync(join(stage, "_up_"), { recursive: true, force: true }); if (!freshResources) cpSync(artifacts.resources, join(stage, "_up_"), { recursive: true });
    prepareStage(stage, artifacts);
    for (const name of removeLegacyExecutables) { checkName(name); rmSync(join(stage, name), { force: true }); }
    cpSync(artifacts.binary, join(stage, executableName)); cpSync(artifacts.library, join(stage, "im_board_lib.dll"));
    inspectTree(stage, { cleanMetadata: true }); validateStage(stage);
    const candidate = join(workspace, "candidate.zip"), verification = createPortableArchive(stage, candidate, join(scratch, "verified"), { run, platform, env });
    validateStage(verification.verifiedStage);
    renameSync(candidate, output); // Same filesystem, previous valid archive survives every earlier failure.
    log(`[windows-package] ${output}`);
    return { output, manifest: verification.manifest };
  } finally {
    rmSync(scratch, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
    rmSync(workspace, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
  }
}
