import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

test("DWS native stderr preserves exit codes without weakening security cmdlet failures", (t) => {
  const shell = process.env.IMBOARD_TEST_POWERSHELL || (process.platform === "win32" ? "powershell.exe" : "pwsh");
  const script = fileURLToPath(new URL("./test-dws-native.ps1", import.meta.url));
  const result = spawnSync(shell, ["-NoLogo", "-NoProfile", "-NonInteractive", "-File", script], { encoding: "utf8", timeout: 20000 });
  if (result.error?.code === "ENOENT" && process.platform !== "win32") { t.skip("PowerShell is optional on non-Windows hosts"); return; }
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr || result.stdout);
  const report = JSON.parse(result.stdout.trim());
  for (const key of ["stderrPreserved", "zeroExitPreserved", "nonzeroExitPreserved", "launchFailureRejected", "cmdletStopPreserved"]) assert.equal(report[key], true, key);
});
