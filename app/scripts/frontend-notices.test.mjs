import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, renameSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { collectFrontendNotices, generateFrontendNotices, outputRelativePath, publishFrontendNotices, verifyFrontendNotices } from "./frontend-notices.mjs";

const scripts = dirname(fileURLToPath(import.meta.url));
const mit = readFileSync(join(scripts, "application-license-sources/frontend/tauri-apps-plugin-dialog-2.7.0/LICENSE_MIT.txt"));
const apache = readFileSync(join(scripts, "application-license-sources/frontend/tauri-apps-plugin-dialog-2.7.0/LICENSE_APACHE-2.0.txt"));
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");
const json = (path, value) => writeFileSync(path, JSON.stringify(value, null, 2) + "\n");

function fixture(run) {
  const appRoot = realpathSync(mkdtempSync(join(tmpdir(), "imboard-frontend-notices-")));
  const root = { name: "fixture", version: "1.0.0", dependencies: { fixture: "1.0.0" } };
  const lock = { name: "fixture", version: "1.0.0", lockfileVersion: 3, packages: { "": root } };
  json(join(appRoot, "package.json"), root);
  function add(path, options = {}) {
    const name = options.name ?? path.split("node_modules/").at(-1);
    const version = options.version ?? "1.0.0";
    const license = options.license ?? "MIT";
    const directory = join(appRoot, path);
    mkdirSync(directory, { recursive: true });
    json(join(directory, "package.json"), { name, version, license });
    writeFileSync(join(directory, "index.js"), "export const value = 1;\n");
    if (options.text !== null) writeFileSync(join(directory, "LICENSE"), options.text ?? mit);
    lock.packages[path] = { version, license, resolved: `https://registry.npmjs.org/${name}/-/${version}.tgz`, integrity: "sha512-fixture", ...options.lock };
  }
  function saveLock() { json(join(appRoot, "package-lock.json"), lock); }
  add("node_modules/fixture"); saveLock();
  const output = join(appRoot, outputRelativePath);
  const collect = () => collectFrontendNotices({ appRoot });
  const generate = () => generateFrontendNotices({ appRoot });
  const verify = (extra = {}) => verifyFrontendNotices({ appRoot, ...extra });
  function supplement(texts, extras = {}) {
    const source = join(appRoot, "scripts/application-license-sources/frontend");
    mkdirSync(source, { recursive: true });
    const files = texts.map(([name, bytes]) => {
      writeFileSync(join(source, name), bytes);
      return { path: name, sourceUrl: `https://raw.githubusercontent.com/example/upstream/0123456789012345678901234567890123456789/${name}`, sha256: digest(bytes), scope: "fixture source" };
    });
    json(join(source, "sources.json"), { schemaVersion: 1, packages: [{ name: "fixture", version: "1.0.0", integrity: "sha512-fixture", licenseDeclaration: lock.packages["node_modules/fixture"].license, texts: files, ...extras }] });
  }
  try { run({ appRoot, output, lock, add, saveLock, collect, generate, verify, supplement }); }
  finally { rmSync(appRoot, { recursive: true, force: true }); }
}

test("generation is deterministic and preserves original complete text bytes", () => fixture(({ output, generate, verify }) => {
  const result = generate();
  assert.equal(result.summary.packageCount, 1);
  assert.equal(result.summary.unresolvedCount, 0);
  assert.equal(result.packages[0].viteBundleInclusion, "not-established");
  const first = readFileSync(join(output, "inventory.json"));
  assert.deepEqual(readFileSync(join(output, result.packages[0].texts[0].path)), mit);
  generate(); verify(); assert.deepEqual(readFileSync(join(output, "inventory.json")), first);
}));

test("non-dev nodes include peer and types candidates without claiming Vite linkage", () => fixture(({ add, saveLock, generate }) => {
  add("node_modules/build-only", { lock: { dev: true } });
  add("node_modules/@types/example", { lock: { peer: true } }); saveLock();
  const result = generate();
  assert.equal(result.summary.packageCount, 2);
  assert.equal(result.packages.find((entry) => entry.name === "@types/example").scope, "non-dev candidate: type metadata");
  assert.equal(result.packages.some((entry) => entry.name === "build-only"), false);
}));

test("recursive vendored LICENSE and NOTICE files are preserved but code names are excluded", () => fixture(({ appRoot, generate }) => {
  const directory = join(appRoot, "node_modules/fixture/vendor/embedded"); mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, "LICENSE.txt"), mit);
  writeFileSync(join(directory, "NOTICE"), "Embedded component copyright\n");
  writeFileSync(join(directory, "copyright.js"), "not a license\n");
  const result = generate();
  assert.equal(result.summary.textCount, 3);
  assert.ok(result.packages[0].texts.some((entry) => entry.sourcePath === "vendor/embedded/NOTICE"));
}));

test("nested node_modules are separate components and never attributed to their parent", () => fixture(({ add, saveLock, generate }) => {
  add("node_modules/fixture/node_modules/child"); saveLock();
  const result = generate();
  assert.equal(result.summary.packageCount, 2);
  assert.equal(result.summary.textCount, 2);
  assert.equal(result.packages[0].texts.some((entry) => entry.sourcePath.includes("node_modules/")), false);
}));

test("declaration-only SPDX stays registered but cannot satisfy full terms", () => fixture(({ appRoot, collect, generate, supplement }) => {
  rmSync(join(appRoot, "node_modules/fixture/LICENSE"));
  writeFileSync(join(appRoot, "node_modules/fixture/LICENSE.spdx"), "SPDXVersion: SPDX-2.1\nPackageLicenseDeclared: MIT\n");
  const incomplete = collect();
  assert.equal(incomplete.inventory.packages[0].texts[0].evidence, "declaration-only");
  assert.match(incomplete.inventory.unresolved[0].reason, /missing package-level complete/);
  assert.throws(generate, /Unresolved frontend licenses/);
  supplement([["LICENSE_MIT", mit]]);
  const result = generate(); assert.equal(result.summary.textCount, 2);
}));

test("all declared alternatives require real complete texts", () => fixture(({ lock, saveLock, appRoot, supplement, generate }) => {
  lock.packages["node_modules/fixture"].license = "MIT OR Apache-2.0"; saveLock();
  json(join(appRoot, "node_modules/fixture/package.json"), { name: "fixture", version: "1.0.0", license: "MIT OR Apache-2.0" });
  assert.throws(generate, /missing package-level complete license terms: Apache-2.0/);
  supplement([["LICENSE_APACHE-2.0", apache]]);
  assert.deepEqual(generate().packages[0].detectedLicenseTerms, ["Apache-2.0", "MIT"]);
}));

test("unknown declarations and unknown custom LICENSE text remain unresolved", () => fixture(({ lock, saveLock, appRoot, collect, generate }) => {
  lock.packages["node_modules/fixture"].license = "LicenseRef-Unknown"; saveLock();
  json(join(appRoot, "node_modules/fixture/package.json"), { name: "fixture", version: "1.0.0", license: "LicenseRef-Unknown" });
  writeFileSync(join(appRoot, "node_modules/fixture/LICENSE"), "Ask the author for permission.\n");
  const result = collect(); assert.equal(result.inventory.unresolved.length, 2);
  assert.throws(generate, /Unresolved frontend licenses/);
}));

test("missing package metadata and license text cannot pass generation", () => fixture(({ appRoot, generate }) => {
  rmSync(join(appRoot, "node_modules/fixture/LICENSE")); assert.throws(generate, /missing package-level complete/);
  rmSync(join(appRoot, "node_modules/fixture/package.json")); assert.throws(generate, /ENOENT/);
}));

test("installed name, version, license and hidden lock metadata must match", () => fixture(({ appRoot, generate, lock }) => {
  const path = join(appRoot, "node_modules/fixture/package.json");
  for (const change of [{ name: "other" }, { version: "2.0.0" }, { license: "ISC" }]) {
    json(path, { name: "fixture", version: "1.0.0", license: "MIT", ...change });
    assert.throws(generate, /mismatch/);
  }
  json(path, { name: "fixture", version: "1.0.0", license: "MIT" });
  json(join(appRoot, "node_modules/.package-lock.json"), { packages: { "node_modules/fixture": { ...lock.packages["node_modules/fixture"], integrity: "sha512-wrong" } } });
  assert.throws(generate, /Installed lock metadata mismatch/);
}));

test("lock hash changes, even dev-only changes, fail verification until regeneration", () => fixture(({ appRoot, generate, verify, lock, saveLock }) => {
  generate(); lock.packages["node_modules/uninstalled-build-tool"] = { version: "9.0.0", dev: true }; saveLock();
  assert.throws(verify, /bytes differ/); generate(); verify();
  writeFileSync(join(appRoot, "package-lock.json"), readFileSync(join(appRoot, "package-lock.json")) + "\n");
  assert.throws(verify, /bytes differ/);
}));

test("source code mutation fails verification even when version and LICENSE are unchanged", () => fixture(({ appRoot, generate, verify }) => {
  generate(); writeFileSync(join(appRoot, "node_modules/fixture/index.js"), "export const value = 2;\n");
  assert.throws(verify, /bytes differ/);
}));

test("missing, tampered and unexpected delivery files all fail verification", () => fixture(({ output, generate, verify }) => {
  const inventory = generate(); const license = join(output, inventory.packages[0].texts[0].path);
  writeFileSync(license, "tampered"); assert.throws(verify, /bytes differ/);
  generate(); rmSync(license); assert.throws(verify, /missing or unexpected/);
  generate(); writeFileSync(join(output, "unexpected.txt"), "pollution"); assert.throws(verify, /missing or unexpected/);
}));

test("supplement source corruption and lock integrity drift are rejected before publication", () => fixture(({ appRoot, generate, supplement, lock, saveLock, output }) => {
  supplement([["LICENSE_MIT", mit]]); generate(); const before = readFileSync(join(output, "inventory.json"));
  writeFileSync(join(appRoot, "scripts/application-license-sources/frontend/LICENSE_MIT"), "corrupt");
  assert.throws(generate, /Supplemental license hash mismatch/);
  assert.deepEqual(readFileSync(join(output, "inventory.json")), before);
  supplement([["LICENSE_MIT", mit]]); lock.packages["node_modules/fixture"].integrity = "sha512-new"; saveLock();
  assert.throws(generate, /Supplement no longer matches locked package/);
}));

test("required packaged vendoring texts cannot disappear during regeneration", () => fixture(({ appRoot, generate, supplement }) => {
  supplement([["LICENSE_MIT", mit]], { requiredPackagedTexts: [{ path: "LICENSE", sha256: digest(mit) }] });
  generate(); rmSync(join(appRoot, "node_modules/fixture/LICENSE")); assert.throws(generate, /ENOENT/);
}));

test("license symlinks and unsafe supplemental paths are rejected", () => fixture(({ appRoot, generate, supplement }) => {
  const license = join(appRoot, "node_modules/fixture/LICENSE"); rmSync(license);
  writeFileSync(join(appRoot, "outside-license"), mit); symlinkSync(join(appRoot, "outside-license"), license);
  assert.throws(generate, /Symlink/); rmSync(license); writeFileSync(license, mit);
  supplement([["LICENSE_MIT", mit]]);
  const path = join(appRoot, "scripts/application-license-sources/frontend/sources.json");
  const data = JSON.parse(readFileSync(path)); data.packages[0].texts[0].path = "../outside-license"; json(path, data);
  assert.throws(generate, /Unsafe relative path/);
}));

test("binary, non-UTF8 and oversized license payloads cannot pass", () => fixture(({ appRoot, generate }) => {
  const license = join(appRoot, "node_modules/fixture/LICENSE");
  writeFileSync(license, Buffer.concat([mit, Buffer.from([0])])); assert.throws(generate, /Binary content/);
  writeFileSync(license, Buffer.from([0xff])); assert.throws(generate, /encoded data/);
  writeFileSync(license, Buffer.alloc(2 * 1024 * 1024 + 1)); assert.throws(generate, /oversized/);
}));

test("failed staged directory publication restores the previous complete generation", () => fixture(({ output, generate, collect, appRoot, verify }) => {
  generate(); const before = readFileSync(join(output, "inventory.json"));
  writeFileSync(join(appRoot, "node_modules/fixture/index.js"), "changed\n");
  let calls = 0;
  assert.throws(() => publishFrontendNotices(collect(), output, { rename(source, destination) {
    calls++; if (calls === 2) throw new Error("injected rename failure"); renameSync(source, destination);
  } }), /injected rename failure/);
  assert.deepEqual(readFileSync(join(output, "inventory.json")), before);
  assert.equal(existsSync(`${output}.generation.lock`), false);
  writeFileSync(join(appRoot, "node_modules/fixture/index.js"), "export const value = 1;\n"); verify();
}));

test("parallel publication lock refuses a second writer without changing output", () => fixture(({ output, generate, collect }) => {
  generate(); const before = readFileSync(join(output, "inventory.json"));
  writeFileSync(`${output}.generation.lock`, "active writer\n");
  assert.throws(() => publishFrontendNotices(collect(), output), /EEXIST/);
  assert.deepEqual(readFileSync(join(output, "inventory.json")), before);
}));

test("unresolved generation preserves an existing complete output", () => fixture(({ output, generate, appRoot }) => {
  generate(); const before = readFileSync(join(output, "inventory.json"));
  writeFileSync(join(appRoot, "node_modules/fixture/LICENSE"), "MIT\n"); assert.throws(generate, /Unresolved frontend licenses/);
  assert.deepEqual(readFileSync(join(output, "inventory.json")), before);
}));

test("artifact verification checks the entire shipped directory against live inputs", () => fixture(({ appRoot, generate, verify }) => {
  generate(); const result = collectFrontendNotices({ appRoot }); const artifactDir = join(appRoot, "dist/third-party/application/frontend");
  publishFrontendNotices(result, artifactDir); verify({ artifactDir });
  writeFileSync(join(artifactDir, "THIRD-PARTY-NOTICES.txt"), "shortened"); assert.throws(() => verify({ artifactDir }), /bytes differ/);
}));

test("collect and verify CLI are separate; verify cannot silently regenerate stale output", () => fixture(({ appRoot, output }) => {
  for (const file of ["frontend-notices.mjs", "collect-frontend-notices.mjs", "verify-frontend-notices.mjs"]) {
    mkdirSync(join(appRoot, "scripts"), { recursive: true }); copyFileSync(join(scripts, file), join(appRoot, "scripts", file));
  }
  const run = (file) => spawnSync(process.execPath, [join(appRoot, "scripts", file)], { encoding: "utf8" });
  assert.equal(run("collect-frontend-notices.mjs").status, 0);
  assert.equal(run("verify-frontend-notices.mjs").status, 0);
  const before = readFileSync(join(output, "inventory.json"));
  writeFileSync(join(appRoot, "package-lock.json"), readFileSync(join(appRoot, "package-lock.json")) + "\n");
  assert.equal(run("verify-frontend-notices.mjs").status, 1);
  assert.deepEqual(readFileSync(join(output, "inventory.json")), before);
}));
