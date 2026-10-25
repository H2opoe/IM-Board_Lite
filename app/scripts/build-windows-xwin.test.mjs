import { test } from "node:test";
import assert from "node:assert/strict";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawn, spawnSync } from "node:child_process";

const script = join(dirname(fileURLToPath(import.meta.url)), "build-windows-xwin.mjs");
const options = { skip: process.platform === "win32" };

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "imboard-xwin-fixture-"));
  const app = join(root, "source tree with spaces", "edition", "app");
  const sdk = join(root, "sdk cache with spaces");
  const llvm = join(root, "llvm tools with spaces");
  const temp = join(root, "temp");
  const bin = join(root, "bin");
  for (const directory of [join(app, "scripts"), join(app, "src-tauri"), temp, bin]) mkdirSync(directory, { recursive: true });
  for (const name of ["clang-cl", "lld-link", "llvm-lib", "llvm-rc"]) {
    mkdirSync(join(llvm, "bin"), { recursive: true });
    writeFileSync(join(llvm, "bin", name), "fixture tool");
  }
  for (const relative of ["crt/include/vcruntime.h", "sdk/include/ucrt/assert.h", "crt/lib/x86_64/vcruntime.lib", "sdk/lib/um/x86_64/kernel32.Lib", "sdk/lib/ucrt/x86_64/ucrt.lib"]) {
    const path = join(sdk, "xwin", relative);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, "fixture SDK input");
  }
  const program = join(app, "scripts/build-windows-xwin.mjs");
  copyFileSync(script, program);
  copyFileSync(join(dirname(script), "release-build-env.mjs"), join(app,"scripts/release-build-env.mjs"));
  // Node stands in for Cargo; its first argument is this owned fixture program.
  symlinkSync(process.execPath, join(bin, "cargo"));
  writeFileSync(join(app, "src-tauri/build"), `
    const fs=require('node:fs');
    const output=process.env.BUILD_PROBE_OUTPUT;
    const includes=process.env.CFLAGS_x86_64_pc_windows_msvc.split(' ');
    const short=includes.find(value=>value.endsWith('/cache/xwin/crt/include'));
    fs.writeFileSync(output,JSON.stringify({args:process.argv.slice(2),short,flags:process.env.CARGO_ENCODED_RUSTFLAGS}));
    if(process.env.BUILD_PROBE_HOLD){
      while(!fs.existsSync(process.env.BUILD_PROBE_HOLD)) Atomics.wait(new Int32Array(new SharedArrayBuffer(4)),0,0,10);
      if(!fs.existsSync(short)) process.exit(81);
    }
    process.exit(Number(process.env.BUILD_PROBE_STATUS||0));
  `);
  for (const name of ["im-board-xwin-edition", "im-board-xwin-cache-edition"]) {
    mkdirSync(join(temp, name));
    writeFileSync(join(temp, name, "foreign.txt"), "preserve existing content");
  }
  const env = { ...process.env, TMPDIR: temp, TMP: temp, TEMP: temp, IMBOARD_LLVM_DIR: llvm, XWIN_CACHE_DIR: sdk, PATH: bin };
  return { root, app, sdk, temp, bin, program, env, close() { rmSync(root, { recursive: true, force: true }); } };
}

function intact(f) {
  assert.deepEqual(readdirSync(f.temp).sort(), ["im-board-xwin-cache-edition", "im-board-xwin-edition"]);
  for (const name of readdirSync(f.temp)) assert.equal(readFileSync(join(f.temp, name, "foreign.txt"), "utf8"), "preserve existing content");
}

test("cross build preserves preexisting temp directories and passes locked flags with spaced SDK paths", options, () => {
  const f = fixture();
  try {
    const output = join(f.root, "output.json");
    const result = spawnSync(process.execPath, [f.program], { env: { ...f.env, BUILD_PROBE_OUTPUT: output }, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    const value = JSON.parse(readFileSync(output));
    assert.equal(value.args[0], "--locked");
    assert.match(value.flags, /sdk cache with spaces/);
    assert.equal(existsSync(value.short), false);
    intact(f);
  } finally { f.close(); }
});

test("a failed compiler result cleans only its own workspace", options, () => {
  const f = fixture();
  try {
    const result = spawnSync(process.execPath, [f.program], { env: { ...f.env, BUILD_PROBE_OUTPUT: join(f.root, "output.json"), BUILD_PROBE_STATUS: "37" }, encoding: "utf8" });
    assert.equal(result.status, 37, result.stderr);
    intact(f);
  } finally { f.close(); }
});

test("failure to spawn Cargo still releases the owned scratch workspace", options, () => {
  const f = fixture();
  try {
    rmSync(join(f.bin, "cargo"));
    const result = spawnSync(process.execPath, [f.program], { env: f.env, encoding: "utf8" });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /cargo 无法启动/);
    intact(f);
  } finally { f.close(); }
});

test("a missing SDK header is rejected before creating a build workspace", options, () => {
  const f = fixture();
  try {
    rmSync(join(f.sdk, "xwin/sdk/include/ucrt/assert.h"));
    const result = spawnSync(process.execPath, [f.program], { env: f.env, encoding: "utf8" });
    assert.equal(result.status, 1);
    assert.match(result.stderr, /assert\.h/);
    intact(f);
  } finally { f.close(); }
});

test("concurrent builds from the same tree do not remove another build's links", options, async () => {
  const f = fixture();
  const hold = join(f.root, "release-first");
  let first;
  try {
    const output = join(f.root, "first.json");
    first = spawn(process.execPath, [f.program], { env: { ...f.env, BUILD_PROBE_OUTPUT: output, BUILD_PROBE_HOLD: hold }, stdio: "ignore" });
    const exited = new Promise((resolve, reject) => { first.once("exit", resolve); first.once("error", reject); });
    for (let attempt = 0; !existsSync(output) && attempt < 100; attempt++) await new Promise(resolve => setTimeout(resolve, 10));
    assert.ok(existsSync(output), "first build reached Cargo");
    const firstValue = JSON.parse(readFileSync(output));
    const second = spawnSync(process.execPath, [f.program], { env: { ...f.env, BUILD_PROBE_OUTPUT: join(f.root, "second.json") }, encoding: "utf8" });
    assert.equal(second.status, 0, second.stderr);
    assert.ok(existsSync(firstValue.short));
    writeFileSync(hold, "done");
    assert.equal(await exited, 0);
    intact(f);
  } finally {
    writeFileSync(hold, "done");
    if (first && first.exitCode === null) { await new Promise(resolve => first.once("exit", resolve)); }
    f.close();
  }
});

test("a TMPDIR containing spaces still yields unsplit C include arguments", options, () => {
  const f = fixture();
  try {
    const spaced = join(f.root, "temp with spaces");
    mkdirSync(spaced);
    const output = join(f.root, "spaced-output.json");
    const result = spawnSync(process.execPath, [f.program], { env: { ...f.env, TMPDIR: spaced, TMP: spaced, TEMP: spaced, BUILD_PROBE_OUTPUT: output }, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    const value = JSON.parse(readFileSync(output));
    assert.ok(value.short);
    assert.equal(/\s/.test(value.short), false);
    assert.equal(existsSync(value.short), false);
    assert.deepEqual(readdirSync(spaced), []);
    intact(f);
  } finally { f.close(); }
});
