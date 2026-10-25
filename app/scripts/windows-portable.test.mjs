import { test } from "node:test";
import assert from "node:assert/strict";
import { cpSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { inspectTree, packagePortable, readArchiveEntries, resolveTargetArtifacts } from "./windows-portable.mjs";

const write = (file, content) => { mkdirSync(dirname(file), { recursive: true }); writeFileSync(file, content); };
function fixture() {
  const root = mkdtempSync(join(tmpdir(), "portable-test-")), appRoot = join(root, "app"), cargo = join(root, "cargo output");
  const release = join(cargo, "x86_64-pc-windows-msvc", "release");
  write(join(release, "im-board.exe"), "new raw cargo executable"); write(join(release, "deps", "im_board_lib.dll"), "new library");
  write(join(release, "_up_", "runtime", "resource.txt"), "current runtime");
  const template = join(root, "artifacts", "tmp", "release-windows-portable", "fixture", "IM-Board");
  write(join(template, "template.txt"), "keep template"); write(join(template, "_up_", "stale.txt"), "stale resource");
  write(join(template, ".hidden"), "hidden resource"); write(join(template, "许可.txt"), "unicode resource");
  write(join(template, ".DS_Store"), "cache"); mkdirSync(join(template, "empty"));
  const output = join(root, "artifacts", "windows", "IM-Board_3.0.0-fixture_windows_x64_portable.zip"); write(output, "previous release");
  const foreign = join(root, "artifacts", "tmp", "windows-portable", "someone-else.txt"); write(foreign, "another build");
  const config = { appRoot, productName: "Fixture Portable", releaseLabel: "3.0.0-fixture", templateVariant: "fixture", executableName: "Fixture.exe", binaryNames: ["im-board.exe", "Fixture.exe"] };
  return { root, appRoot, cargo, release, template, config, output, foreign, options: { env: { ...process.env, CARGO_TARGET_DIR: cargo }, log: () => {} } };
}
function isolated(body) { return context => { const f = fixture(); try { return body(f, context); } finally { rmSync(f.root, { recursive: true, force: true }); } }; }
function oldPreserved(f) { assert.equal(readFileSync(f.output, "utf8"), "previous release"); assert.equal(readFileSync(f.foreign, "utf8"), "another build"); assert.deepEqual(readdirSync(dirname(f.output)).filter(name => name.startsWith(".portable-build-")), []); }
function central(buffer) { const end = buffer.length - 22; return { end, offset: buffer.readUInt32LE(end + 16), size: buffer.readUInt32LE(end + 12), count: buffer.readUInt16LE(end + 10) }; }
function firstFileOffset(buffer) { let p = central(buffer).offset; while (buffer[p + 46 + buffer.readUInt16LE(p + 28) - 1] === 47) p += 46 + buffer.readUInt16LE(p + 28) + buffer.readUInt16LE(p + 30) + buffer.readUInt16LE(p + 32); return p; }
function filenameCrc(buffer) { let crc = 0xffffffff; for (const byte of buffer) { crc ^= byte; for (let i = 0; i < 8; i++) crc = crc & 1 ? 0xedb88320 ^ (crc >>> 1) : crc >>> 1; } return (crc ^ 0xffffffff) >>> 0; }
function unicodePathArchive(original, placement, replacement, alter = () => {}) {
  const cd = central(original), target = firstFileOffset(original), records = [];
  for (let p = cd.offset, index = 0; index < cd.count; index++) {
    const length = 46 + original.readUInt16LE(p + 28) + original.readUInt16LE(p + 30) + original.readUInt16LE(p + 32);
    records.push({ p, bytes: Buffer.from(original.subarray(p, p + length)), offset: original.readUInt32LE(p + 42) }); p += length;
  }
  const name = original.subarray(target + 46, target + 46 + original.readUInt16LE(target + 28)), value = Buffer.alloc(5 + Buffer.byteLength(replacement));
  value[0] = 1; value.writeUInt32LE(filenameCrc(name), 1); Buffer.from(replacement).copy(value, 5); alter(value);
  const field = Buffer.alloc(value.length + 4); field.writeUInt16LE(0x7075); field.writeUInt16LE(value.length, 2); value.copy(field, 4);
  const offsets = new Map(), local = [], sorted = [...records].sort((a, b) => a.offset - b.offset); let position = sorted[0].offset;
  local.push(original.subarray(0, position));
  for (let i = 0; i < sorted.length; i++) {
    const record = sorted[i], next = sorted[i + 1]?.offset ?? cd.offset; offsets.set(record.offset, position);
    let bytes = original.subarray(record.offset, next);
    if (record.p === target && placement !== "central") {
      const header = Buffer.from(bytes.subarray(0, 30)), end = 30 + header.readUInt16LE(26) + header.readUInt16LE(28);
      header.writeUInt16LE(header.readUInt16LE(28) + field.length, 28); bytes = Buffer.concat([header, bytes.subarray(30, end), field, bytes.subarray(end)]);
    }
    local.push(bytes); position += bytes.length;
  }
  const directory = records.map(record => {
    let bytes = record.bytes; bytes.writeUInt32LE(offsets.get(record.offset), 42);
    if (record.p === target && placement !== "local") {
      const end = 46 + bytes.readUInt16LE(28) + bytes.readUInt16LE(30), header = Buffer.from(bytes.subarray(0, 46));
      header.writeUInt16LE(header.readUInt16LE(30) + field.length, 30); bytes = Buffer.concat([header, bytes.subarray(46, end), field, bytes.subarray(end)]);
    }
    return bytes;
  });
  const end = Buffer.from(original.subarray(cd.end)); end.writeUInt32LE(position, 16); end.writeUInt32LE(directory.reduce((sum, b) => sum + b.length, 0), 12);
  return { bytes: Buffer.concat([...local, ...directory, end]), originalName: name.toString("utf8") };
}

test("absolute Cargo target and raw executable with deps library are selected", isolated(f => {
  const result = resolveTargetArtifacts(f.appRoot, f.config.binaryNames, f.options.env);
  assert.equal(result.binary, join(f.release, "im-board.exe")); assert.equal(result.library, join(f.release, "deps", "im_board_lib.dll"));
}));
test("relative Cargo target resolves from the Cargo project directory", isolated(f => {
  const relative = join(f.appRoot, "src-tauri", "relative output"); cpSync(f.cargo, relative, { recursive: true });
  assert.equal(resolveTargetArtifacts(f.appRoot, f.config.binaryNames, { CARGO_TARGET_DIR: "relative output" }).release, join(relative, "x86_64-pc-windows-msvc", "release"));
}));
test("default target and compatibility executable are supported", isolated(f => {
  const target = join(f.appRoot, "src-tauri", "target"); cpSync(f.cargo, target, { recursive: true });
  const release = join(target, "x86_64-pc-windows-msvc", "release"); rmSync(join(release, "im-board.exe")); write(join(release, "Fixture.exe"), "compatibility executable");
  assert.equal(resolveTargetArtifacts(f.appRoot, f.config.binaryNames, {}).binary, join(release, "Fixture.exe"));
}));
test("real archive keeps template, new binary, complete resources, hidden/unicode and empty directories", isolated(f => {
  const result = packagePortable(f.config, f.options), entries = readArchiveEntries(result.output);
  const names = entries.map(entry => entry.name);
  for (const file of ["Fixture.exe", "im_board_lib.dll", "template.txt", ".hidden", "许可.txt", "_up_/runtime/resource.txt"]) assert.ok(names.includes(`Fixture Portable/${file}`));
  assert.ok(names.includes("Fixture Portable/empty/")); assert.ok(!names.some(name => name.includes("stale.txt") || name.includes(".DS_Store")));
  assert.equal(readFileSync(join(f.template, "_up_", "stale.txt"), "utf8"), "stale resource");
  assert.equal(readFileSync(f.foreign, "utf8"), "another build"); assert.deepEqual(readdirSync(dirname(f.output)).filter(name => name.startsWith(".portable-build-")), []);
}));
test("compression failure preserves old archive and unrelated staging", isolated(f => {
  assert.throws(() => packagePortable(f.config, { ...f.options, platform: "darwin", run: () => ({ status: 7, stderr: "fixture compressor failed" }) }), /zip 打包失败/); oldPreserved(f);
}));
test("unavailable compression tool fails without publishing", isolated(f => {
  assert.throws(() => packagePortable(f.config, { ...f.options, platform: "darwin", run: () => ({ status: null, error: new Error("ENOENT") }) }), /ENOENT/); oldPreserved(f);
}));
test("unavailable Unix verification tool fails instead of skipping archive verification", { skip: process.platform === "win32" }, isolated(f => {
  assert.throws(() => packagePortable(f.config, { ...f.options, platform: "darwin", run(command, args, options) { return command === "unzip" ? { status: null, error: new Error("missing verifier") } : spawnSync(command, args, options); } }), /missing verifier/); oldPreserved(f);
}));
test("prepare-stage failure preserves input and prior archive", isolated(f => {
  assert.throws(() => packagePortable({ ...f.config, prepareStage() { throw new Error("prepare failed"); } }, f.options), /prepare failed/); oldPreserved(f);
  assert.equal(readFileSync(join(f.release, "_up_", "runtime", "resource.txt"), "utf8"), "current runtime");
}));
test("stage boundary rejection happens before compression", isolated(f => {
  let compressed = false;
  assert.throws(() => packagePortable({ ...f.config, validateStage() { throw new Error("edition boundary"); } }, { ...f.options, run() { compressed = true; } }), /edition boundary/);
  assert.equal(compressed, false); oldPreserved(f);
}));
test("extracted boundary validation is required before replacing the old archive", isolated(f => {
  let calls = 0;
  assert.throws(() => packagePortable({ ...f.config, validateStage() { if (++calls === 2) throw new Error("extracted boundary"); } }, f.options), /extracted boundary/);
  assert.equal(calls, 2); oldPreserved(f);
}));
test("a resource mutation during compression fails the CRC and manifest comparison", isolated(f => {
  assert.throws(() => packagePortable(f.config, { ...f.options, run(command, args, options) {
    if (command === "zip") write(join(options.cwd, f.config.productName, "template.txt"), "changed resource");
    return spawnSync(command, args, options);
  } }), /清单或完整性不一致/); oldPreserved(f);
}));
test("corrupt compressed data fails Unix integrity validation before publishing", { skip: process.platform === "win32" }, isolated(f => {
  assert.throws(() => packagePortable(f.config, { ...f.options, platform: "darwin", run(command, args, options) {
    const result = spawnSync(command, args, options);
    if (command === "zip" && result.status === 0) { const file = args[3], b = readFileSync(file), p = firstFileOffset(b), local = b.readUInt32LE(p + 42); const data = local + 30 + b.readUInt16LE(local + 26) + b.readUInt16LE(local + 28); b[data] ^= 0xff; writeFileSync(file, b); }
    return result;
  } }), /unzip 失败/); oldPreserved(f);
}));
test("Windows PowerShell fallback uses fixed encoded scripts and environment paths, preserving hidden data", { skip: process.platform === "win32" }, isolated(f => {
  f.config.productName = "Fixture ' `$;";
  const calls = [];
  const result = packagePortable(f.config, { ...f.options, platform: "win32", run(command, args, options) {
    if (command === "zip") return { status: null, error: new Error("zip unavailable") };
    assert.equal(command, "powershell.exe"); const script = Buffer.from(args.at(-1), "base64").toString("utf16le"); calls.push(script);
    assert.ok(!script.includes(f.root)); assert.ok(args.includes("-NonInteractive"));
    const env = options.env;
    if (script.includes("ZipArchiveMode]::Create")) return spawnSync("zip", ["-X", "-q", "-r", env.IM_BOARD_PORTABLE_ARCHIVE, basename(env.IM_BOARD_PORTABLE_STAGE)], { cwd: dirname(env.IM_BOARD_PORTABLE_STAGE), encoding: "utf8" });
    assert.ok(script.includes("ExtractToDirectory")); return spawnSync("unzip", ["-q", env.IM_BOARD_PORTABLE_ARCHIVE, "-d", env.IM_BOARD_PORTABLE_VERIFY], { encoding: "utf8" });
  } });
  assert.equal(calls.length, 2); assert.ok(readArchiveEntries(result.output).some(entry => entry.name.endsWith("/.hidden")));
}));
test("overlapping builds own independent staging and publish complete archives", isolated(f => {
  const seen = [];
  packagePortable({ ...f.config, prepareStage(outer) {
    seen.push(outer);
    packagePortable({ ...f.config, prepareStage(inner) {
      seen.push(inner);
      assert.notEqual(inner, outer);
      assert.equal(readdirSync(dirname(f.output)).filter(name => name.startsWith(".portable-build-")).length, 2);
      assert.equal(readFileSync(join(outer, "template.txt"), "utf8"), "keep template");
    } }, f.options);
    assert.equal(readFileSync(join(outer, "template.txt"), "utf8"), "keep template");
  } }, f.options);
  assert.equal(seen.length, 2); assert.ok(readArchiveEntries(f.output).some(entry => entry.name.endsWith("/Fixture.exe")));
  assert.equal(readFileSync(f.foreign, "utf8"), "another build");
  assert.deepEqual(readdirSync(dirname(f.output)).filter(name => name.startsWith(".portable-build-")), []);
}));
test("archive traversal is rejected before the Unix extraction tool runs", { skip: process.platform === "win32" }, isolated(f => {
  let extracted = false;
  assert.throws(() => packagePortable(f.config, { ...f.options, platform: "darwin", run(command, args, options) {
    if (command === "unzip") extracted = true;
    const result = spawnSync(command, args, options);
    if (command === "zip" && result.status === 0) {
      const file = args[3], bytes = readFileSync(file), p = firstFileOffset(bytes), length = bytes.readUInt16LE(p + 28);
      Buffer.from("../" + "x".repeat(length - 3)).copy(bytes, p + 46); writeFileSync(file, bytes);
    }
    return result;
  } }), /路径不能安全/);
  assert.equal(extracted, false); oldPreserved(f);
}));
test("PowerShell failure keeps an existing archive", isolated(f => {
  assert.throws(() => packagePortable(f.config, { ...f.options, platform: "win32", run: command => ({ status: command === "zip" ? null : 9, stderr: "native fixture failed" }) }), /powershell.exe 失败/); oldPreserved(f);
}));
test("input links are rejected before following any external target", { skip: process.platform === "win32" }, isolated(f => {
  const external = join(f.root, "external.txt"); write(external, "outside source"); symlinkSync(external, join(f.template, "link.txt"));
  assert.throws(() => packagePortable(f.config, f.options), /禁止链接/); oldPreserved(f); assert.equal(readFileSync(external, "utf8"), "outside source");
}));
test("case aliases cannot create conflicting files on Windows", isolated((f, context) => {
  write(join(f.template, "Alias.txt"), "one"); write(join(f.template, "alias.txt"), "two");
  if (readFileSync(join(f.template, "Alias.txt"), "utf8") === "two") { context.skip("fixture filesystem cannot create distinct case aliases"); return; }
  assert.throws(() => inspectTree(f.template), /大小写路径冲突/);
}));
test("ZIP central directory case aliases are rejected on every fixture filesystem", isolated(f => {
  packagePortable(f.config, f.options); const original = readFileSync(f.output), cd = central(original), p = firstFileOffset(original);
  const length = 46 + original.readUInt16LE(p + 28) + original.readUInt16LE(p + 30) + original.readUInt16LE(p + 32);
  const alias = Buffer.from(original.subarray(p, p + length)), nameLength = alias.readUInt16LE(28);
  const aliasName = alias.subarray(46, 46 + nameLength).toString("utf8").toUpperCase();
  assert.equal(Buffer.byteLength(aliasName), nameLength); Buffer.from(aliasName).copy(alias, 46);
  const end = Buffer.from(original.subarray(cd.end)); end.writeUInt16LE(cd.count + 1, 8); end.writeUInt16LE(cd.count + 1, 10); end.writeUInt32LE(cd.size + alias.length, 12);
  writeFileSync(f.output, Buffer.concat([original.subarray(0, cd.end), alias, end]));
  assert.throws(() => readArchiveEntries(f.output), /重复或大小写冲突/);
}));
test("traversal and invalid release identifiers are rejected", isolated(f => {
  assert.throws(() => packagePortable({ ...f.config, releaseLabel: "../../../bad" }, f.options), /发行版本无效/); oldPreserved(f);
  for (const name of ['invalid"quote', "invalid?wildcard", "CON.txt", "invalid."]) assert.throws(() => packagePortable({ ...f.config, productName: name }, f.options), /归档路径/);
}));
test("archive links and central/local filename disagreements are rejected before extraction", isolated(f => {
  packagePortable(f.config, f.options); const original = readFileSync(f.output), p = firstFileOffset(original);
  const link = Buffer.from(original); link.writeUInt32LE(0xa1ff * 0x10000, p + 38); writeFileSync(f.output, link);
  assert.throws(() => readArchiveEntries(f.output), /禁止链接/);
  const mismatch = Buffer.from(original), local = mismatch.readUInt32LE(p + 42); mismatch[local + 30] ^= 1; writeFileSync(f.output, mismatch);
  assert.throws(() => readArchiveEntries(f.output), /本地文件名/);
}));
test("valid ZIP64 end metadata can describe an ordinary small archive", isolated(f => {
  packagePortable(f.config, f.options); const original = readFileSync(f.output), cd = central(original);
  const end = Buffer.from(original.subarray(cd.end)), z64 = Buffer.alloc(56), locator = Buffer.alloc(20);
  z64.writeUInt32LE(0x06064b50); z64.writeBigUInt64LE(44n, 4); z64.writeBigUInt64LE(BigInt(cd.count), 24); z64.writeBigUInt64LE(BigInt(cd.count), 32); z64.writeBigUInt64LE(BigInt(cd.size), 40); z64.writeBigUInt64LE(BigInt(cd.offset), 48);
  locator.writeUInt32LE(0x07064b50); locator.writeBigUInt64LE(BigInt(cd.end), 8); locator.writeUInt32LE(1, 16);
  end.writeUInt16LE(0xffff, 8); end.writeUInt16LE(0xffff, 10); end.writeUInt32LE(0xffffffff, 12); end.writeUInt32LE(0xffffffff, 16);
  writeFileSync(f.output, Buffer.concat([original.subarray(0, cd.end), z64, locator, end]));
  assert.equal(readArchiveEntries(f.output).length, cd.count);
}));
for (const placement of ["local", "central", "both"]) test(`Unicode path traversal in ${placement} extra is rejected before any verifier or extraction runs`, { skip: process.platform === "win32" }, isolated(f => {
  const sentinel = join(f.root, "artifacts", "outside.txt"); write(sentinel, "protected outside sentinel"); let unzipCalled = false;
  assert.throws(() => packagePortable(f.config, { ...f.options, platform: "darwin", run(command, args, options) {
    if (command === "unzip") unzipCalled = true;
    const result = spawnSync(command, args, options);
    if (command === "zip" && result.status === 0) writeFileSync(args[3], unicodePathArchive(readFileSync(args[3]), placement, "../../../outside.txt").bytes);
    return result;
  } }), /Unicode 路径与原文件名不一致/);
  assert.equal(unzipCalled, false); assert.equal(readFileSync(sentinel, "utf8"), "protected outside sentinel"); oldPreserved(f);
}));
test("valid matching local and central Unicode path extras remain compatible", { skip: process.platform === "win32" }, isolated(f => {
  packagePortable(f.config, { ...f.options, run(command, args, options) {
    const result = spawnSync(command, args, options);
    if (command === "zip" && result.status === 0) {
      const original = readFileSync(args[3]), p = firstFileOffset(original), name = original.subarray(p + 46, p + 46 + original.readUInt16LE(p + 28)).toString("utf8");
      writeFileSync(args[3], unicodePathArchive(original, "both", name).bytes);
    }
    return result;
  } });
  assert.ok(readArchiveEntries(f.output).some(entry => entry.name.endsWith("/Fixture.exe")));
}));
test("Unicode path version and original filename CRC must be valid in local and central extras", isolated(f => {
  packagePortable(f.config, f.options); const original = readFileSync(f.output), p = firstFileOffset(original), name = original.subarray(p + 46, p + 46 + original.readUInt16LE(p + 28)).toString("utf8");
  for (const placement of ["local", "central"]) for (const change of [value => { value[0] = 2; }, value => { value[1] ^= 1; }]) {
    writeFileSync(f.output, unicodePathArchive(original, placement, name, change).bytes); assert.throws(() => readArchiveEntries(f.output), /版本或文件名 CRC 无效/);
  }
}));
test("ordinary Info-ZIP timestamp and UID metadata remain compatible", { skip: process.platform === "win32" }, isolated(f => {
  packagePortable(f.config, { ...f.options, run(command, args, options) { return spawnSync(command, command === "zip" ? args.filter(arg => arg !== "-X") : args, options); } });
  assert.ok(readArchiveEntries(f.output).some(entry => entry.name.endsWith("/许可.txt")));
}));

test("fresh releases never inherit old template state or require an old template", isolated(f => {
  write(join(f.template,"app.sqlite"),"SYNTHETIC_PRIVATE_DATABASE");
  const result=packagePortable({...f.config,freshTemplate:true},f.options);
  let names=readArchiveEntries(result.output).map(entry=>entry.name);
  assert.ok(!names.some(name=>name.includes("app.sqlite") || name.includes("template.txt") || name.includes(".hidden")));
  rmSync(f.template,{recursive:true,force:true});
  assert.ok(packagePortable({...f.config,freshTemplate:true},f.options).output);
}));

test("output folder Finder metadata stays outside the payload and verified tree", isolated(f => {
  const result = packagePortable(f.config, { ...f.options, run(command, args, options) {
    if (command === "zip") {
      assert.ok(!options.cwd.startsWith(dirname(f.output)));
      const workspaces = readdirSync(dirname(f.output)).filter(name => name.startsWith(".portable-build-"));
      for (const name of workspaces) write(join(dirname(f.output), name, ".DS_Store"), "Finder metadata");
    }
    return spawnSync(command, args, options);
  } });
  assert.ok(!readArchiveEntries(result.output).some(entry => entry.name.endsWith("/.DS_Store")));
}));
