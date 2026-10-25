import { createHash, randomUUID } from "node:crypto";
import {
  closeSync, existsSync, lstatSync, mkdirSync, openSync, readFileSync,
  readdirSync, renameSync, rmSync, writeFileSync
} from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { TextDecoder } from "node:util";

export const defaultAppRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const outputRelativePath = "public/third-party/application/frontend";
const sourceRelativePath = "scripts/application-license-sources/frontend";
const decoder = new TextDecoder("utf-8", { fatal: true });
const licenseFile = /^(?:licen[cs]e|copying|copyright|notice|patents)(?:$|[._ -])/i;
const codeExtension = /\.(?:[cm]?js|[cm]?ts|tsx|jsx|map|rs|py|c|cpp|h|go|json)$/i;
const knownLicenses = new Set(["MIT", "ISC", "BSD-2-Clause", "BSD-3-Clause", "Apache-2.0"]);
const scope = "All package-lock v3 non-dev nodes are conservative frontend dependency candidates, including types and peer dependencies. This is not the Vite linked module graph or a license compatibility determination. Build-only dev nodes, Rust, CLI, models and Python are outside this inventory.";
const sha256 = (data) => createHash("sha256").update(data).digest("hex");
const jsonBytes = (value) => Buffer.from(JSON.stringify(value, null, 2) + "\n");

function safeRelative(value) {
  if (typeof value !== "string" || !value || value.includes("\\") || value.startsWith("/")
      || value.split("/").some((part) => !part || part === "." || part === "..")) {
    throw new Error(`Unsafe relative path: ${String(value)}`);
  }
  return value;
}

function checkPath(path) {
  const absolute = resolve(path);
  let current = absolute;
  while (true) {
    try {
      if (lstatSync(current).isSymbolicLink()) throw new Error(`Symlink is not permitted: ${current}`);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
    const parent = dirname(current);
    if (parent === current) break;
    current = parent;
  }
  return absolute;
}

function readFile(path, maxBytes = 32 * 1024 * 1024) {
  checkPath(path);
  const info = lstatSync(path);
  if (!info.isFile() || info.size > maxBytes) throw new Error(`Unsafe or oversized input: ${path}`);
  return readFileSync(path);
}

function readJson(path) {
  return JSON.parse(decoder.decode(readFile(path)));
}

function listFiles(directory, excludeNodeModules = false) {
  checkPath(directory);
  if (!lstatSync(directory).isDirectory()) throw new Error(`Not a directory: ${directory}`);
  const files = [];
  function visit(path) {
    for (const entry of readdirSync(path, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name, "en"))) {
      if (excludeNodeModules && entry.name === "node_modules" && entry.isDirectory()) continue;
      const child = join(path, entry.name);
      if (entry.isSymbolicLink()) throw new Error(`Symlink is not permitted: ${child}`);
      if (entry.isDirectory()) visit(child);
      else if (entry.isFile()) files.push(child);
      else throw new Error(`Unsupported filesystem entry: ${child}`);
    }
  }
  visit(directory);
  return files;
}

function detectTerms(text) {
  const normalized = text.replace(/\s+/g, " ").toLowerCase();
  const ids = [];
  if (normalized.includes("permission is hereby granted, free of charge")
      && normalized.includes("the above copyright notice and this permission notice")
      && normalized.includes("the software is provided") && /warrant(?:y|ies)/.test(normalized)) ids.push("MIT");
  if (normalized.includes("permission to use, copy, modify, and/or distribute")
      && normalized.includes("copyright notice and this permission notice")
      && normalized.includes("the author disclaims all warranties")) ids.push("ISC");
  if (normalized.includes("redistribution and use in source and binary forms")
      && normalized.includes("redistributions of source code must retain")
      && normalized.includes("redistributions in binary form must reproduce")
      && normalized.includes("this software is provided") && normalized.includes("disclaimed")) {
    ids.push(normalized.includes("neither the name") ? "BSD-3-Clause" : "BSD-2-Clause");
  }
  if (normalized.includes("apache license") && normalized.includes("version 2.0, january 2004")
      && normalized.includes("grant of copyright license") && normalized.includes("grant of patent license")
      && normalized.includes("redistribution") && normalized.includes("limitation of liability")
      && normalized.includes("end of terms and conditions")) ids.push("Apache-2.0");
  return ids;
}

function licenseEvidence(path, bytes) {
  const text = decoder.decode(bytes);
  if (text.includes("\0")) throw new Error(`Binary content in license text: ${path}`);
  const filename = path.split("/").at(-1);
  const detected = detectTerms(text);
  const declaration = /\bSPDXVersion\s*:|\bSPDX-License-Identifier\s*:/.test(text) && detected.length === 0;
  const notice = /^(?:notice|copyright|patents)(?:$|[._ -])/i.test(filename);
  return {
    sha256: sha256(bytes), bytes: bytes.length, detectedLicenseTerms: detected,
    evidence: detected.length ? "license-terms" : declaration ? "declaration-only" : notice ? "copyright-or-notice" : "unrecognized-license-text"
  };
}

function expectedTerms(declaration) {
  if (typeof declaration !== "string" || !declaration.trim()) return { ids: [], unknown: ["missing license declaration"] };
  const tokens = declaration.replace(/[()]/g, " ").trim().split(/\s+/).filter((token) => token !== "AND" && token !== "OR");
  return { ids: [...new Set(tokens.filter((token) => knownLicenses.has(token)))], unknown: tokens.filter((token) => !knownLicenses.has(token)) };
}

function packageName(lockPath) {
  if (!/^(?:node_modules\/(?:@[a-zA-Z0-9._-]+\/)?[a-zA-Z0-9._-]+)(?:\/node_modules\/(?:@[a-zA-Z0-9._-]+\/)?[a-zA-Z0-9._-]+)*$/.test(lockPath)) {
    throw new Error(`Unsupported lock node path: ${lockPath}`);
  }
  return lockPath.split("node_modules/").at(-1);
}

function loadSupplements(appRoot) {
  const sourceRoot = join(appRoot, sourceRelativePath);
  const manifestPath = join(sourceRoot, "sources.json");
  if (!existsSync(manifestPath)) return { packages: [], inputs: [] };
  const manifestBytes = readFile(manifestPath);
  const manifest = JSON.parse(decoder.decode(manifestBytes));
  if (manifest.schemaVersion !== 1 || !Array.isArray(manifest.packages)) throw new Error("Invalid supplemental source manifest");
  const inputs = [{ path: `${sourceRelativePath}/sources.json`, sha256: sha256(manifestBytes) }];
  const seen = new Set();
  for (const entry of manifest.packages) {
    const key = `${entry.name}@${entry.version}`;
    if (seen.has(key) || !entry.integrity || !Array.isArray(entry.texts)) throw new Error(`Invalid or duplicate supplemental source: ${key}`);
    seen.add(key);
    for (const text of entry.texts) {
      const sourcePath = safeRelative(text.path);
      if (!/^https:\/\/raw\.githubusercontent\.com\//.test(text.sourceUrl ?? "")) throw new Error(`Missing pinned source URL: ${sourcePath}`);
      const bytes = readFile(join(sourceRoot, sourcePath), 2 * 1024 * 1024);
      if (sha256(bytes) !== text.sha256) throw new Error(`Supplemental license hash mismatch: ${sourcePath}`);
      text.bytes = bytes;
      inputs.push({ path: `${sourceRelativePath}/${sourcePath}`, sha256: text.sha256 });
    }
  }
  return { packages: manifest.packages, inputs: inputs.sort((a, b) => a.path.localeCompare(b.path, "en")) };
}

export function collectFrontendNotices({ appRoot = defaultAppRoot } = {}) {
  appRoot = checkPath(appRoot);
  const lockBytes = readFile(join(appRoot, "package-lock.json"));
  const packageBytes = readFile(join(appRoot, "package.json"));
  const lock = JSON.parse(decoder.decode(lockBytes));
  const appPackage = JSON.parse(decoder.decode(packageBytes));
  if (lock.lockfileVersion !== 3 || !lock.packages || !lock.packages[""]) throw new Error("package-lock v3 packages are required");
  const sortedEntries = (value) => Object.entries(value ?? {}).sort(([a], [b]) => a.localeCompare(b, "en"));
  if (JSON.stringify(sortedEntries(lock.packages[""].dependencies)) !== JSON.stringify(sortedEntries(appPackage.dependencies))) throw new Error("package.json runtime dependencies differ from the lock root");
  if (lock.packages[""].name !== appPackage.name || lock.packages[""].version !== appPackage.version) throw new Error("package.json name/version differs from the lock root");
  const installedLockPath = join(appRoot, "node_modules/.package-lock.json");
  const installedLock = existsSync(installedLockPath) ? readJson(installedLockPath) : undefined;
  const supplements = loadSupplements(appRoot);
  const packages = [], unresolved = [], files = new Map();
  const sections = ["IM-Board frontend third-party license texts\n\n", scope + "\n",
    "Registry integrity values below are taken from package-lock; installed package bytes are fingerprinted separately. Collection does not download or authenticate registry tarballs. Full terms are detected structurally; this is not a legal review. All declared alternatives and discovered internal LICENSE/NOTICE files are preserved.\n"];
  for (const [lockPath, node] of Object.entries(lock.packages).filter(([path, entry]) => path && !entry.dev).sort(([a], [b]) => a.localeCompare(b, "en"))) {
    const name = packageName(lockPath);
    if (node.link || !node.version || !node.integrity || !node.resolved) throw new Error(`Incomplete registry lock metadata: ${lockPath}`);
    const directory = join(appRoot, lockPath);
    const metadataBytes = readFile(join(directory, "package.json"));
    const metadata = JSON.parse(decoder.decode(metadataBytes));
    if (metadata.name !== name || metadata.version !== node.version) throw new Error(`Installed package name/version mismatch: ${lockPath}`);
    if (metadata.license !== node.license) throw new Error(`Installed package license declaration mismatch: ${lockPath}`);
    if (installedLock) {
      const installed = installedLock.packages?.[lockPath];
      if (!installed || installed.version !== node.version || installed.integrity !== node.integrity) throw new Error(`Installed lock metadata mismatch: ${lockPath}`);
    }
    const id = sha256(Buffer.from(lockPath)).slice(0, 16);
    const sourceFiles = listFiles(directory, true);
    const tree = createHash("sha256"), texts = [], rootTerms = new Set(), allTerms = new Set();
    let sourceBytes = 0;
    function addText(sourcePath, bytes, origin, additional = {}) {
      const outputPath = `licenses/${id}/${origin === "package" ? "package" : "upstream"}/${safeRelative(sourcePath)}`;
      if (files.has(outputPath)) throw new Error(`Duplicate output text path: ${outputPath}`);
      const evidence = licenseEvidence(sourcePath, bytes);
      const entry = { path: outputPath, sourcePath, origin, ...additional, ...evidence };
      files.set(outputPath, bytes); texts.push(entry);
      for (const term of evidence.detectedLicenseTerms) {
        allTerms.add(term);
        if (origin === "upstream" || !sourcePath.includes("/")) rootTerms.add(term);
      }
      if (evidence.evidence === "unrecognized-license-text") unresolved.push({ package: `${name}@${node.version}`, path: lockPath, sourcePath, reason: "license file has no recognized complete terms; declaration or custom terms require review" });
      return entry;
    }
    for (const path of sourceFiles) {
      const sourcePath = relative(directory, path).split(sep).join("/");
      const isLicense = licenseFile.test(path.split(sep).at(-1)) && !codeExtension.test(path);
      const bytes = readFile(path, isLicense ? 2 * 1024 * 1024 : undefined);
      sourceBytes += bytes.length;
      tree.update(`${sourcePath}\0${bytes.length}\0${sha256(bytes)}\n`);
      if (isLicense) addText(sourcePath, bytes, "package");
    }
    const supplement = supplements.packages.find((entry) => entry.name === name && entry.version === node.version);
    if (supplement) {
      if (supplement.integrity !== node.integrity || supplement.licenseDeclaration !== node.license) throw new Error(`Supplement no longer matches locked package: ${name}@${node.version}`);
      for (const required of supplement.requiredPackagedTexts ?? []) {
        if (sha256(readFile(join(directory, safeRelative(required.path)))) !== required.sha256) throw new Error(`Supplement prerequisite changed: ${name}/${required.path}`);
      }
      for (const text of supplement.texts) addText(text.path, text.bytes, "upstream", { sourceUrl: text.sourceUrl, scope: text.scope });
    }
    const expected = expectedTerms(node.license);
    for (const unknown of expected.unknown) unresolved.push({ package: `${name}@${node.version}`, path: lockPath, reason: `unsupported license declaration: ${unknown}` });
    for (const term of expected.ids) {
      if (!rootTerms.has(term) && !(term === "ISC" && allTerms.has(term) && name === "victory-vendor" && node.version === "36.9.2")) {
        unresolved.push({ package: `${name}@${node.version}`, path: lockPath, reason: `missing package-level complete license terms: ${term}` });
      }
    }
    for (const term of supplement?.requiredAdditionalTerms ?? []) {
      if (!rootTerms.has(term)) unresolved.push({ package: `${name}@${node.version}`, path: lockPath, reason: `missing additional component terms: ${term}` });
    }
    if (!texts.length) unresolved.push({ package: `${name}@${node.version}`, path: lockPath, reason: "no license or notice text found" });
    const entry = {
      lockPath, name, version: node.version, resolved: node.resolved, registryIntegrity: node.integrity,
      licenseDeclaration: node.license, optional: Boolean(node.optional), peer: Boolean(node.peer),
      scope: name.startsWith("@types/") || name === "csstype" ? "non-dev candidate: type metadata" : "non-dev frontend dependency candidate",
      directDependency: Object.hasOwn(appPackage.dependencies ?? {}, name), viteBundleInclusion: "not-established",
      installedSource: { packageJsonSha256: sha256(metadataBytes), fileCount: sourceFiles.length, sizeBytes: sourceBytes, treeSha256: tree.digest("hex"), excludes: ["nested node_modules packages are separately inventoried"] },
      detectedLicenseTerms: [...allTerms].sort(), texts
    };
    packages.push(entry);
    sections.push(`\n${"=".repeat(72)}\n${name}@${node.version}\nLock node: ${lockPath}\nDeclared: ${node.license}\nRegistry integrity: ${node.integrity}\n`);
    for (const text of texts) sections.push(`\n--- ${text.origin}: ${text.sourcePath} [${text.evidence}] ---\n${text.sourceUrl ? `Source: ${text.sourceUrl}\n` : ""}SHA-256: ${text.sha256}\n\n${decoder.decode(files.get(text.path))}\n`);
  }
  const inventory = {
    schemaVersion: 1, application: { name: appPackage.name, version: appPackage.version }, scope,
    inputs: { packageLockSha256: sha256(lockBytes), packageJsonSha256: sha256(packageBytes), supplementalSources: supplements.inputs },
    packages, unresolved, summary: { packageCount: packages.length, textCount: packages.reduce((sum, entry) => sum + entry.texts.length, 0), unresolvedCount: unresolved.length }
  };
  const notices = Buffer.from(sections.join(""));
  inventory.notices = { path: "THIRD-PARTY-NOTICES.txt", sha256: sha256(notices), bytes: notices.length };
  files.set("THIRD-PARTY-NOTICES.txt", notices);
  files.set("inventory.json", jsonBytes(inventory));
  if (!readFile(join(appRoot, "package-lock.json")).equals(lockBytes)
      || !readFile(join(appRoot, "package.json")).equals(packageBytes)) {
    throw new Error("Package inputs changed during frontend license collection; retry after dependency changes finish");
  }
  const finalSupplements = loadSupplements(appRoot);
  if (JSON.stringify(finalSupplements.inputs) !== JSON.stringify(supplements.inputs)) {
    throw new Error("Supplemental source inputs changed during frontend license collection");
  }
  return { inventory, files };
}

export function verifyFiles(directory, expectedFiles) {
  const actual = listFiles(directory).map((path) => relative(directory, path).split(sep).join("/")).sort();
  const expected = [...expectedFiles.keys()].sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error(`Notice output contains missing or unexpected files: ${directory}`);
  for (const [path, expectedBytes] of expectedFiles) {
    if (!readFile(join(directory, path)).equals(expectedBytes)) throw new Error(`Notice output bytes differ from current inputs: ${path}`);
  }
}

function requireResolved(result) {
  if (result.inventory.unresolved.length) {
    throw new Error(`Unresolved frontend licenses:\n${JSON.stringify(result.inventory.unresolved, null, 2)}`);
  }
}

export function publishFrontendNotices(result, output, { rename = renameSync } = {}) {
  requireResolved(result);
  output = checkPath(output);
  mkdirSync(dirname(output), { recursive: true });
  const lock = `${output}.generation.lock`;
  const lockFd = openSync(lock, "wx");
  const stage = `${output}.stage-${randomUUID()}`, backup = `${output}.backup-${randomUUID()}`;
  let movedOld = false, published = false;
  try {
    writeFileSync(lockFd, jsonBytes({ pid: process.pid }));
    mkdirSync(stage);
    for (const [path, bytes] of result.files) {
      const target = join(stage, safeRelative(path));
      mkdirSync(dirname(target), { recursive: true });
      writeFileSync(target, bytes, { flag: "wx" });
    }
    verifyFiles(stage, result.files);
    if (existsSync(output)) {
      if (!lstatSync(output).isDirectory()) throw new Error(`Output is not a directory: ${output}`);
      rename(output, backup); movedOld = true;
    }
    rename(stage, output); published = true;
    if (movedOld) rmSync(backup, { recursive: true });
  } catch (error) {
    if (movedOld && !published) renameSync(backup, output);
    throw error;
  } finally {
    rmSync(stage, { recursive: true, force: true });
    closeSync(lockFd); rmSync(lock);
  }
}

export function generateFrontendNotices({ appRoot = defaultAppRoot, output = join(appRoot, outputRelativePath) } = {}) {
  const result = collectFrontendNotices({ appRoot });
  publishFrontendNotices(result, output);
  return result.inventory;
}

export function verifyFrontendNotices({ appRoot = defaultAppRoot, output = join(appRoot, outputRelativePath), artifactDir } = {}) {
  const result = collectFrontendNotices({ appRoot });
  requireResolved(result); verifyFiles(output, result.files);
  if (artifactDir) verifyFiles(artifactDir, result.files);
  return result.inventory;
}

export function parseArguments(argv, allowArtifact = false) {
  const options = {};
  for (let i = 0; i < argv.length; i++) {
    const key = { "--app-root": "appRoot", "--output": "output", ...(allowArtifact ? { "--artifact-dir": "artifactDir" } : {}) }[argv[i]];
    if (!key || !argv[i + 1] || argv[i + 1].startsWith("--")) throw new Error(`Unknown or incomplete option: ${argv[i]}`);
    options[key] = resolve(argv[++i]);
  }
  return options;
}
