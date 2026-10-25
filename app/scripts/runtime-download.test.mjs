import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { ensureRuntimeArchive } from "./runtime-download.mjs";

const content = Buffer.from("verified fixture archive");
const name = "fixture.tar.gz";
const inputs = { schemaVersion: 1, assets: { [name]: { sha256: createHash("sha256").update(content).digest("hex"), sizeBytes: content.length, url: "https://github.com/fixture/runtime/releases/download/v1/fixture.tar.gz" } } };
function fixture() {
  const root = mkdtempSync(join(tmpdir(), "imboard-download-"));
  const path = join(root, name);
  writeFileSync(join(root, "foreign.txt"), "preserve");
  return { root, path, intact() { assert.deepEqual(readdirSync(root).sort(), [name, "foreign.txt"].sort()); assert.equal(readFileSync(join(root, "foreign.txt"), "utf8"), "preserve"); }, close() { rmSync(root, { recursive: true, force: true }); } };
}

test("a cache is reused only when its actual size and SHA match", async () => {
  const f = fixture();
  try {
    writeFileSync(f.path, content);
    await ensureRuntimeArchive(f.path, name, { inputs, fetcher: () => assert.fail("verified cache needs no network") });
    f.intact();
  } finally { f.close(); }
});

test("an invalid cache and a corrupt first source cannot bypass the pinned digest", async () => {
  const f = fixture();
  try {
    writeFileSync(f.path, "unverified existing archive");
    const calls = [];
    await ensureRuntimeArchive(f.path, name, { inputs, fetcher: async url => { calls.push(url); return new Response(calls.length === 1 ? Buffer.alloc(content.length, 88) : content); } });
    assert.equal(calls.length, 2);
    assert.deepEqual(readFileSync(f.path), content);
    f.intact();
  } finally { f.close(); }
});

test("truncated, oversized, and unavailable downloads leave the old cache intact", async () => {
  for (const response of [() => new Response(content.subarray(1)), () => new Response(Buffer.concat([content, content])), () => new Response(null, { status: 503 })]) {
    const f = fixture();
    try {
      writeFileSync(f.path, "old-cache");
      await assert.rejects(ensureRuntimeArchive(f.path, name, { inputs, fetcher: async () => response() }), /无法下载固定归档/);
      assert.equal(readFileSync(f.path, "utf8"), "old-cache");
      f.intact();
    } finally { f.close(); }
  }
});

test("stream failure removes partial downloads while preserving the existing archive", async () => {
  const f = fixture();
  try {
    writeFileSync(f.path, "old-cache");
    await assert.rejects(ensureRuntimeArchive(f.path, name, { inputs, fetcher: async () => new Response(new ReadableStream({ start(controller) { controller.enqueue(content.subarray(0, 3)); controller.error(new Error("fixture stream failed")); } })) }));
    assert.equal(readFileSync(f.path, "utf8"), "old-cache");
    f.intact();
  } finally { f.close(); }
});

test("concurrent verified downloads own separate temporary files", async () => {
  const f = fixture();
  try {
    writeFileSync(f.path, "old-cache");
    let arrived = 0;
    let ready;
    const gate = new Promise(resolve => { ready = resolve; });
    const fetcher = async () => { if (++arrived === 2) ready(); await gate; return new Response(content); };
    await Promise.all([ensureRuntimeArchive(f.path, name, { inputs, fetcher }), ensureRuntimeArchive(f.path, name, { inputs, fetcher })]);
    assert.deepEqual(readFileSync(f.path), content);
    f.intact();
  } finally { f.close(); }
});

test("invalid manifest metadata fails before making any request", async () => {
  const f = fixture();
  try {
    writeFileSync(f.path, "old-cache");
    for (const patch of [{ sha256: "unknown" }, { sizeBytes: 0 }, { sizeBytes: 513 * 1024 * 1024 }, { url: "https://foreign.example/asset" }]) {
      const changed = { ...inputs, assets: { [name]: { ...inputs.assets[name], ...patch } } };
      await assert.rejects(ensureRuntimeArchive(f.path, name, { inputs: changed, fetcher: () => assert.fail("invalid input must not download") }));
    }
    f.intact();
  } finally { f.close(); }
});

test("a symlink or directory cache is rejected without touching its target", { skip: process.platform === "win32" }, async () => {
  const f = fixture();
  try {
    symlinkSync(join(f.root, "foreign.txt"), f.path);
    await assert.rejects(ensureRuntimeArchive(f.path, name, { inputs }), /普通文件/);
    f.intact();
    rmSync(f.path);
    mkdirSync(f.path);
    await assert.rejects(ensureRuntimeArchive(f.path, name, { inputs }), /普通文件/);
    f.intact();
  } finally { f.close(); }
});
