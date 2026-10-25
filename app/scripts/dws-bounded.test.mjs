import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
test("PAT granting preserves output and exit codes and stops its owned process at deadline", (t) => {
  const shell = process.env.IMBOARD_TEST_POWERSHELL || (process.platform === "win32" ? "powershell.exe" : "pwsh");
  const script = fileURLToPath(new URL("./test-dws-bounded.ps1", import.meta.url));
  const result = spawnSync(shell,["-NoLogo","-NoProfile","-File",script],{encoding:"utf8",timeout:30000});
  if (result.error?.code === "ENOENT") return t.skip("PowerShell unavailable; run in Windows CI");
  assert.equal(result.status,0,result.stdout+result.stderr);
  const report=JSON.parse(result.stdout.trim());
  assert.equal(report.deadlineStopsOwnedProcess,true);
  assert.equal(report.exitCodePreserved,true);
});
