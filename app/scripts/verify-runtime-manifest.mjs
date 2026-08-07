import { createHash } from "node:crypto";
import { existsSync, lstatSync, readFileSync, readlinkSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const runtimeRoot = join(appRoot, "runtime");
const runtimeManifest = JSON.parse(readFileSync(join(runtimeRoot, "manifest.json"), "utf8"));
const releaseManifest = JSON.parse(readFileSync(join(appRoot, "release-manifest.json"), "utf8"));

function fail(message) {
  throw new Error(`Runtime manifest verification failed: ${message}`);
}

function isGeneratedPath(name) {
  return name.split(sep).includes("__pycache__") || name.endsWith(".pyc") || name.endsWith(".pyo") || name.endsWith(".DS_Store");
}

function walk(root, directory = root, output = []) {
  if (!existsSync(directory)) fail(`missing component path ${relative(runtimeRoot, directory)}`);
  for (const entry of readdirSync(directory).sort()) {
    const path = join(directory, entry);
    const name = relative(root, path);
    if (isGeneratedPath(name)) fail(`generated/cache file present: ${relative(appRoot, path)}`);
    const info = lstatSync(path);
    if (info.isSymbolicLink()) {
      const target = readlinkSync(path);
      const resolvedTarget = resolve(dirname(path), target);
      if (!resolvedTarget.startsWith(`${resolve(root)}${sep}`)) fail(`symbolic link escapes component: ${relative(appRoot, path)}`);
      output.push({ path, name, size: Buffer.byteLength(target), kind: "symlink", content: target });
    } else if (info.isDirectory()) walk(root, path, output);
    else if (info.isFile()) output.push({ path, name, size: info.size, kind: "file" });
  }
  return output;
}

function componentStats(component) {
  const componentRoot = resolve(runtimeRoot, component.path);
  if (!componentRoot.startsWith(`${resolve(runtimeRoot)}${sep}`)) fail(`component escapes runtime root: ${component.path}`);
  const files = walk(componentRoot);
  const tree = createHash("sha256");
  for (const file of files) {
    const fileSha256 = createHash("sha256").update(file.kind === "symlink" ? file.content : readFileSync(file.path)).digest("hex");
    tree.update(`${file.name.split(sep).join("/")}\\0${file.kind}\\0${file.size}\\0${fileSha256}\\n`);
  }
  return {
    fileCount: files.length,
    sizeBytes: files.reduce((total, file) => total + file.size, 0),
    treeSha256: tree.digest("hex"),
    files,
  };
}

if (runtimeManifest.schemaVersion !== 1) fail("unsupported schemaVersion");
if (runtimeManifest.edition !== releaseManifest.edition || runtimeManifest.target !== releaseManifest.target) {
  fail("edition/target does not match release-manifest.json");
}
if (!Array.isArray(runtimeManifest.components) || runtimeManifest.components.length === 0) fail("components are empty");

const covered = new Set();
for (const component of runtimeManifest.components) {
  if (!component.id || !component.version || !component.platform || !component.path) fail("component metadata is incomplete");
  const actual = componentStats(component);
  for (const key of ["fileCount", "sizeBytes", "treeSha256"]) {
    if (actual[key] !== component[key]) {
      fail(`${component.id}/${component.platform} ${key}: current ${actual[key]}, expected ${component[key]}`);
    }
  }
  for (const file of actual.files) covered.add(resolve(file.path));
}

for (const path of walk(runtimeRoot)) {
  if (path.name === "manifest.json") continue;
  if (!covered.has(resolve(path.path))) fail(`unlisted runtime file: ${relative(appRoot, path.path)}`);
}
if (runtimeManifest.edition === "lite" && runtimeManifest.components.some((component) => component.id !== "llama.cpp")) {
  fail("Lite runtime may only contain llama.cpp");
}

console.log(`Runtime manifest verified: ${runtimeManifest.components.length} components, ${runtimeManifest.edition}/${runtimeManifest.target}.`);
