#!/usr/bin/env node
import { parseArguments, verifyFrontendNotices } from "./frontend-notices.mjs";

try {
  const result = verifyFrontendNotices(parseArguments(process.argv.slice(2), true));
  console.log(JSON.stringify({ status: "verified", ...result.summary, packageLockSha256: result.inputs.packageLockSha256 }));
} catch (error) {
  console.error(error.message); process.exitCode = 1;
}
