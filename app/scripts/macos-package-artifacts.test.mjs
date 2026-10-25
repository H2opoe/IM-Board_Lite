import { test } from "node:test";
import assert from "node:assert/strict";
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, renameSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createVerifiedDmg, repairMacosBundle } from "./macos-package-artifacts.mjs";

const native = { skip: process.platform !== "darwin" };
function required(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  assert.equal(result.status, 0, result.stderr);
  return result;
}
function digest(file) { return createHash("sha256").update(readFileSync(file)).digest("hex"); }
function fixture() {
  const root = mkdtempSync(join(tmpdir(), "imboard-signing-fixture-"));
  const appPath = join(root, "Fixture App.app");
  const helper = join(appPath, "Contents/Frameworks/Helper.app");
  for (const [app, name, id] of [[appPath, "fixture", "local.fixture.original"], [helper, "helper", "local.fixture.helper"]]) {
    mkdirSync(join(app, "Contents/MacOS"), { recursive: true });
    writeFileSync(join(app, "Contents/Info.plist"), `<?xml version="1.0"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>${id}</string><key>CFBundleName</key><string>Fixture</string><key>CFBundleExecutable</key><string>${name}</string><key>CFBundlePackageType</key><string>APPL</string><key>CFBundleShortVersionString</key><string>1.0.0</string><key>CFBundleVersion</key><string>1</string><key>LSRequiresCarbon</key><true/></dict></plist>`);
    copyFileSync("/usr/bin/true", join(app, "Contents/MacOS", name));
  }
  for (const [name, key] of [["outer", "com.apple.security.cs.disable-library-validation"], ["helper", "com.apple.security.cs.allow-jit"]]) writeFileSync(join(root, `${name}.plist`), `<?xml version="1.0"?><plist version="1.0"><dict><key>${key}</key><true/></dict></plist>`);
  required("codesign", ["--force", "--sign", "-", "--entitlements", join(root, "helper.plist"), helper]);
  required("codesign", ["--force", "--sign", "-", "--options", "runtime", "--entitlements", join(root, "outer.plist"), appPath]);
  const config = { bundleId: "local.fixture.repaired", productName: "Fixture App", appVersion: "2.0.0", macosBundleVersion: "200" };
  return { root, appPath, helper, config, close() { rmSync(root, { recursive: true, force: true }); }, clean() { assert.deepEqual(readdirSync(root).sort(), ["Fixture App.app", "helper.plist", "outer.plist"].sort()); } };
}
function entitlements(app) { return required("codesign", ["--display", "--entitlements", ":-", app]).stdout; }

test("outer repair preserves actual nested signatures, entitlements and executable modes", native, () => {
  const f = fixture();
  try {
    const nested = join(f.helper, "Contents/MacOS/helper");
    const before = { nested: digest(nested), outerEntitlements: entitlements(f.appPath), childEntitlements: entitlements(f.helper), mode: statSync(nested).mode };
    repairMacosBundle(f);
    assert.equal(digest(nested), before.nested);
    assert.equal(statSync(nested).mode, before.mode);
    assert.equal(entitlements(f.appPath), before.outerEntitlements);
    assert.equal(entitlements(f.helper), before.childEntitlements);
    assert.equal(required("/usr/libexec/PlistBuddy", ["-c", "Print :CFBundleIdentifier", join(f.appPath, "Contents/Info.plist")]).stdout.trim(), f.config.bundleId);
    required("codesign", ["--verify", "--deep", "--strict", f.appPath]);
    f.clean();
  } finally { f.close(); }
});

test("failed signing keeps the original plist and binary unchanged", native, () => {
  const f = fixture();
  try {
    const plist = join(f.appPath, "Contents/Info.plist"), exe = join(f.appPath, "Contents/MacOS/fixture");
    const before = [digest(plist), digest(exe)];
    const run = (command, args, options) => command === "codesign" ? { status: 61, stderr: "fixture signing failure" } : spawnSync(command, args, options);
    assert.throws(() => repairMacosBundle(f, { run }), /fixture signing failure/);
    assert.deepEqual([digest(plist), digest(exe)], before);
    f.clean();
  } finally { f.close(); }
});

test("failed publication restores the previous app and removes only owned staging", native, () => {
  const f = fixture();
  try {
    const before = digest(join(f.appPath, "Contents/Info.plist"));
    let calls = 0;
    const rename = (from, to) => { if (++calls === 2) throw new Error("fixture publication failure"); renameSync(from, to); };
    assert.throws(() => repairMacosBundle(f, { rename }), /fixture publication failure/);
    assert.equal(digest(join(f.appPath, "Contents/Info.plist")), before);
    f.clean();
  } finally { f.close(); }
});

test("a failed candidate DMG verification preserves the previous output", native, () => {
  const f = fixture();
  try {
    const dmgPath = join(f.root, "release.dmg");
    writeFileSync(dmgPath, "previous valid artifact");
    const run = (command, args, options) => {
      if (command !== "hdiutil") return spawnSync(command, args, options);
      if (args[0] === "create") { writeFileSync(args.at(-1), "candidate artifact"); return { status: 0 }; }
      return { status: 42, stderr: "fixture verify failure" };
    };
    assert.throws(() => createVerifiedDmg({ ...f, dmgPath, productName: f.config.productName }, { run }), /fixture verify failure/);
    assert.equal(readFileSync(dmgPath, "utf8"), "previous valid artifact");
    assert.equal(readdirSync(f.root).some(name => name.startsWith(".imboard-")), false);
  } finally { f.close(); }
});

test("a verified candidate DMG replaces the output after both tool steps succeed", native, () => {
  const f = fixture();
  try {
    const dmgPath = join(f.root, "release.dmg"), stages = [];
    writeFileSync(dmgPath, "previous artifact");
    const run = (command, args, options) => {
      if (command !== "hdiutil") return spawnSync(command, args, options);
      stages.push(args[0]);
      assert.equal(readFileSync(dmgPath, "utf8"), "previous artifact");
      if (args[0] === "create") writeFileSync(args.at(-1), "verified candidate");
      return { status: 0 };
    };
    createVerifiedDmg({ ...f, dmgPath, productName: f.config.productName }, { run });
    assert.deepEqual(stages, ["create", "verify"]);
    assert.equal(readFileSync(dmgPath, "utf8"), "verified candidate");
    assert.equal(readdirSync(f.root).some(name => name.startsWith(".imboard-")), false);
  } finally { f.close(); }
});
