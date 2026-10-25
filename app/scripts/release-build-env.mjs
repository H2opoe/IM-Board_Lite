import { homedir } from "node:os";
import { resolve, dirname } from "node:path";

// Paths used by the compiler's diagnostics/panic strings are not runtime paths.
// Encode each flag separately so spaces in worktree/cache paths remain intact.
export function releaseBuildEnv(appRoot, input = process.env, extraRoots = []) {
  const env = { ...input };
  const flags = input.CARGO_ENCODED_RUSTFLAGS
    ? input.CARGO_ENCODED_RUSTFLAGS.split("\x1f").filter(Boolean)
    : (input.RUSTFLAGS || "").split(/\s+/).filter(Boolean);
  const mappings = [
    [dirname(resolve(appRoot, "..")), "/workspace"],
    [resolve(appRoot, ".."), "/src/im-board"],
    [homedir(), "/home/build"],
    ...[input.CARGO_HOME, input.RUSTUP_HOME].filter(Boolean).map(path => [resolve(path), "/build/toolchain"]),
    ...extraRoots.map(path => [resolve(path), "/src/im-board/app"]),
  ];
  for (const [from, to] of mappings) flags.push(`--remap-path-prefix=${from}=${to}`);
  env.CARGO_ENCODED_RUSTFLAGS = [...new Set(flags)].join("\x1f");
  delete env.RUSTFLAGS;
  return env;
}
