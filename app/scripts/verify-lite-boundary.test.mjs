import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, copyFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

test("history checks only the requested reachable graph and preserves old tags", () => {
  const root = mkdtempSync(join(tmpdir(), "imboard-history-"));
  function git(...args) {
    const result = spawnSync("git", ["-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid", ...args], { cwd: root, encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  }
  try {
    git("init", "-b", "legacy");
    mkdirSync(join(root, "app/src"), { recursive: true });
    writeFileSync(join(root, "app/src/wechat.ts"), "export const fixture = true;");
    git("add", "."); git("commit", "-m", "historical full fixture"); git("tag", "old-release");
    git("checkout", "--orphan", "public-lite"); git("rm", "-rf", ".");
    mkdirSync(join(root, "app/scripts"), { recursive: true });
    mkdirSync(join(root, "app/src"), { recursive: true });
    copyFileSync(new URL("./verify-lite-boundary.mjs", import.meta.url), join(root, "app/scripts/verify-lite-boundary.mjs"));
    copyFileSync(new URL("./lite-enterprise-domains.mjs", import.meta.url), join(root,"app/scripts/lite-enterprise-domains.mjs"));
    writeFileSync(join(root, "app/src/index.ts"), "export const fixture = true;");
    git("add", "."); git("commit", "-m", "clean public fixture");
    const verify = (...args) => spawnSync(process.execPath, [join(root, "app/scripts/verify-lite-boundary.mjs"), "--history", "--ref", ...args], { encoding: "utf8" });
    assert.equal(verify("public-lite").status, 0);
    assert.notEqual(verify("old-release").status, 0);
    assert.notEqual(verify("missing-ref").status, 0);
    assert.notEqual(verify("--all").status, 0);
    assert.equal(git("tag", "--list"), "old-release");
  } finally { rmSync(root, { recursive: true, force: true }); }
});
