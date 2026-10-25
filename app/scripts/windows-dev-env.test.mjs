import { test } from "node:test";
import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { cargoTargetDirectory, environmentFromOutput, runWithVisualStudio } from "./windows-dev-env.mjs";

function fixture() {
  const root = mkdtempSync(join(tmpdir(), "imboard-vs-fixture-"));
  const appRoot = join(root, "source with spaces", "app");
  const temporaryRoot = join(root, "temp with spaces");
  mkdirSync(appRoot, { recursive: true });
  mkdirSync(temporaryRoot);
  writeFileSync(join(temporaryRoot, "foreign.cmd"), "preserve");
  const env = { ...process.env, CARGO_TARGET_DIR: join(root, "target with spaces") };
  const options = { appRoot, temporaryRoot, vsDevCmd: "C:\\Build Tools\\Common7\\Tools\\VsDevCmd.bat", command: "node", args: [], env };
  return { root, options, close() { rmSync(root, { recursive: true, force: true }); }, intact() { assert.deepEqual(readdirSync(temporaryRoot), ["foreign.cmd"]); assert.equal(readFileSync(join(temporaryRoot, "foreign.cmd"), "utf8"), "preserve"); } };
}

test("application arguments reach Node literally and never enter the CMD bootstrap", () => {
  const f = fixture();
  try {
    const output = join(f.root, "output.json");
    const probe = join(f.options.appRoot, "probe.js");
    writeFileSync(probe, 'require("node:fs").writeFileSync(process.argv[2],JSON.stringify({args:process.argv.slice(3),target:process.env.CARGO_TARGET_DIR}));process.exit(19);');
    const literal = ['quote"value', "& echo unexpected", "%USERNAME%", "!variable!", "line\nvalue"];
    f.options.args = [probe, output, ...literal];
    let batch;
    f.options.spawn = (command, args, options) => {
      if (command !== "cmd.exe") return spawnSync(command, args, options);
      const path = args.at(-1).slice(2, -2);
      batch = readFileSync(path, "utf8");
      assert.ok(existsSync(path));
      return { status: 0, stdout: `Path=${process.env.PATH}\r\nVCToolsInstallDir=fixture\r\n` };
    };
    assert.equal(runWithVisualStudio(f.options), 19);
    assert.deepEqual(JSON.parse(readFileSync(output)), { args: literal, target: f.options.env.CARGO_TARGET_DIR });
    for (const arg of literal) assert.equal(batch.includes(arg), false);
    assert.equal(batch.includes(f.options.appRoot), false);
    f.intact();
  } finally { f.close(); }
});

test("setup failures and missing native commands preserve foreign files and clean owned work", () => {
  const f = fixture();
  try {
    for (const setup of [{ error: new Error("spawn failure") }, { status: 17, stderr: "fixture compiler failure" }, { status: 0, stdout: "BROKEN=1" }]) {
      f.options.spawn = () => setup;
      assert.throws(() => runWithVisualStudio(f.options));
      f.intact();
    }
    f.options.command = "missing-fixture-native";
    f.options.spawn = command => command === "cmd.exe" ? { status: 0, stdout: "PATH=fixture" } : { error: new Error("ENOENT") };
    assert.throws(() => runWithVisualStudio(f.options), /ENOENT/);
    f.intact();
  } finally { f.close(); }
});

test("nested calls retain separate bootstrap files", () => {
  const f = fixture();
  try {
    let nesting = false;
    f.options.spawn = (command, args) => {
      if (command !== "cmd.exe") return { status: 0 };
      const path = args.at(-1).slice(2, -2);
      assert.ok(existsSync(path));
      if (!nesting) {
        nesting = true;
        assert.equal(runWithVisualStudio(f.options), 0);
        assert.ok(existsSync(path));
      }
      return { status: 0, stdout: "PATH=fixture" };
    };
    assert.equal(runWithVisualStudio(f.options), 0);
    f.intact();
  } finally { f.close(); }
});

test("different worktrees get different default targets and explicit targets remain supported", () => {
  assert.notEqual(cargoTargetDirectory("C:\\full\\app", {}), cargoTargetDirectory("C:\\lite\\app", {}));
  assert.equal(cargoTargetDirectory("/fixture/app", { CARGO_TARGET_DIR: "../build" }), resolve("/fixture/build"));
});

test("case-insensitive Windows environment overrides replace existing values once", () => {
  const env = environmentFromOutput("Path=fixture\r\ncargo_target_dir=old\r\nTOKEN=a=b\r\n=C:=ignored\r\n", { CARGO_TARGET_DIR: "new" });
  assert.deepEqual(env, { Path: "fixture", CARGO_TARGET_DIR: "new", TOKEN: "a=b" });
});

test("unsafe setup paths and CMD wrappers fail before invoking a command", () => {
  const f = fixture();
  try {
    f.options.spawn = () => assert.fail("no command should run");
    f.options.vsDevCmd = 'C:\\%INJECT%\\VsDevCmd.bat';
    assert.throws(() => runWithVisualStudio(f.options), /批处理字符/);
    f.options.command = "tauri.cmd";
    assert.throws(() => runWithVisualStudio(f.options), /JavaScript/);
    f.intact();
  } finally { f.close(); }
});
