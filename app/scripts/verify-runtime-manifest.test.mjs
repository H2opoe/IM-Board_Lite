import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, copyFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";

function fixture(run) {
  const root = mkdtempSync(join(tmpdir(), "imboard-runtime-test-"));
  try {
    mkdirSync(join(root, "scripts"));
    mkdirSync(join(root, "runtime/llama"), { recursive: true });
    copyFileSync(new URL("./verify-runtime-manifest.mjs", import.meta.url), join(root, "scripts/verify.mjs"));
    const payload = Buffer.from("synthetic executable bytes");
    const sha = createHash("sha256").update(payload).digest("hex");
    const tree = createHash("sha256").update(`server\\0file\\0${payload.length}\\0${sha}\\n`).digest("hex");
    writeFileSync(join(root, "release-manifest.json"), JSON.stringify({ edition: "lite", target: "macos" }));
    writeFileSync(join(root, "runtime/manifest.json"), JSON.stringify({ schemaVersion: 1, edition: "lite", target: "macos", components: [{ id: "llama.cpp", version: "fixture", platform: "fixture", path: "llama", fileCount: 1, sizeBytes: payload.length, treeSha256: tree }] }));
    writeFileSync(join(root, "runtime/llama/server"), payload);
    const verify = (...args) => spawnSync(process.execPath, [join(root, "scripts/verify.mjs"), ...args], { encoding: "utf8" });
    run({ root, payload, sha, verify });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test("strict runtime verification checks real bytes on every host", () => fixture(({ root, verify }) => {
  assert.equal(verify().status, 0);
  writeFileSync(join(root, "runtime/llama/server"), "tampered");
  assert.notEqual(verify().status, 0);
  rmSync(join(root, "runtime/llama/server"));
  assert.notEqual(verify().status, 0);
}));

test("LFS pointers never prove release payload availability", () => fixture(({ root, payload, sha, verify }) => {
  writeFileSync(join(root, "runtime/llama/server"), `version https://git-lfs.github.com/spec/v1\noid sha256:${sha}\nsize ${payload.length}\n`);
  const strict = verify();
  assert.notEqual(strict.status, 0);
  assert.match(strict.stderr, /Git LFS payload is missing/);
  const metadata = verify("--metadata-only");
  assert.equal(metadata.status, 0);
  assert.match(metadata.stdout, /payload NOT verified/);
}));
