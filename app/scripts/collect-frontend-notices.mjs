#!/usr/bin/env node
import { generateFrontendNotices, parseArguments } from "./frontend-notices.mjs";

try {
  const result = generateFrontendNotices(parseArguments(process.argv.slice(2)));
  console.log(JSON.stringify({ status: "generated", ...result.summary, packageLockSha256: result.inputs.packageLockSha256 }));
} catch (error) {
  console.error(error.message); process.exitCode = 1;
}
