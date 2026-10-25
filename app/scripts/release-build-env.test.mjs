import test from "node:test";
import assert from "node:assert/strict";
import { homedir } from "node:os";
import { resolve } from "node:path";
import { releaseBuildEnv } from "./release-build-env.mjs";

test("release remapping preserves encoded flags and space-containing roots without changing runtime environment",()=>{
 const input={CARGO_ENCODED_RUSTFLAGS:"-Clinker-flavor=lld-link\x1f-Lnative=/fixture/sdk lib",CARGO_HOME:"/fixture/build cache",API_RUNTIME:"kept"};
 const env=releaseBuildEnv("/fixture/work tree/app",input,["/fixture/link root"]);
 const flags=env.CARGO_ENCODED_RUSTFLAGS.split("\x1f");
 assert.ok(flags.includes("-Lnative=/fixture/sdk lib"));
 assert.ok(flags.includes(`--remap-path-prefix=${resolve("/fixture/work tree")}=/src/im-board`));
 assert.ok(flags.includes(`--remap-path-prefix=${homedir()}=/home/build`));
 assert.ok(flags.includes(`--remap-path-prefix=${resolve("/fixture/link root")}=/src/im-board/app`));
 assert.equal(env.API_RUNTIME,"kept");assert.equal(input.RUSTFLAGS,undefined);
});
test("plain existing flags survive conversion and input stays untouched",()=>{
 const input={RUSTFLAGS:"-C debuginfo=0"};const env=releaseBuildEnv("/fixture/app",input);
 assert.deepEqual(env.CARGO_ENCODED_RUSTFLAGS.split("\x1f").slice(0,2),["-C","debuginfo=0"]);
 assert.equal(env.RUSTFLAGS,undefined);assert.equal(input.RUSTFLAGS,"-C debuginfo=0");
});
