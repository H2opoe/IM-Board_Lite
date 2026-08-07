import { existsSync, mkdirSync, rmSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const defaultLlvmDir = join(appRoot, "..", "..", "toolchains", "llvm-macos-arm64", "LLVM-22.1.0-macOS-ARM64");
const llvmDir = process.env.IMBOARD_LLVM_DIR || defaultLlvmDir;
const llvmBinDir = join(llvmDir, "bin");
const sourceCacheRoot = process.env.XWIN_CACHE_DIR || join(appRoot, "..", "..", "toolchains", "cargo-xwin-x64");
const requiredPaths = [
  join(llvmBinDir, "clang-cl"),
  join(llvmBinDir, "lld-link"),
  join(llvmBinDir, "llvm-lib"),
  join(sourceCacheRoot, "xwin", "crt", "lib", "x86_64", "vcruntime.lib"),
  join(sourceCacheRoot, "xwin", "sdk", "lib", "um", "x86_64", "kernel32.Lib"),
  join(sourceCacheRoot, "xwin", "sdk", "lib", "ucrt", "x86_64", "ucrt.lib"),
];

function fail(message) {
  console.error(`[windows-xwin] ${message}`);
  process.exit(1);
}

for (const path of requiredPaths) {
  if (!existsSync(path)) {
    fail(`cargo-xwin 缓存不完整：${path}`);
  }
}

const workspaceName = basename(dirname(appRoot)).replace(/[^a-z0-9_-]+/gi, "-");
// 使用工作树级稳定短路径，既规避外置盘空格拆参，也让 Cargo 指纹可跨构建复用。
const linkRoot = join(tmpdir(), `im-board-xwin-${workspaceName}`);
const cacheLinkRoot = join(tmpdir(), `im-board-xwin-cache-${workspaceName}`);
rmSync(linkRoot, { recursive: true, force: true });
rmSync(cacheLinkRoot, { recursive: true, force: true });
mkdirSync(dirname(linkRoot), { recursive: true });
symlinkSync(appRoot, linkRoot, "dir");
// 外置盘路径包含空格；C 编译器参数通过临时符号链接引用真实缓存，避免 cc-rs 拆参失败。
symlinkSync(sourceCacheRoot, cacheLinkRoot, "dir");

const sep = "\u001f";
const crtLib = join(cacheLinkRoot, "xwin", "crt", "lib", "x86_64");
const sdkUmLib = join(cacheLinkRoot, "xwin", "sdk", "lib", "um", "x86_64");
const sdkUcrtLib = join(cacheLinkRoot, "xwin", "sdk", "lib", "ucrt", "x86_64");
const includeArgs = [
  join(cacheLinkRoot, "xwin", "crt", "include"),
  join(cacheLinkRoot, "xwin", "sdk", "include", "ucrt"),
  join(cacheLinkRoot, "xwin", "sdk", "include", "um"),
  join(cacheLinkRoot, "xwin", "sdk", "include", "shared"),
  join(cacheLinkRoot, "xwin", "sdk", "include", "winrt"),
];
const cFlags = [
  "--target=x86_64-pc-windows-msvc",
  "-Wno-unused-command-line-argument",
  "-fuse-ld=lld-link",
  ...includeArgs.flatMap((path) => ["/imsvc", path]),
].join(" ");

const env = {
  ...process.env,
  AR_x86_64_pc_windows_msvc: "llvm-lib",
  CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER: "lld-link",
  CC_x86_64_pc_windows_msvc: "clang-cl",
  CXX_x86_64_pc_windows_msvc: "clang-cl",
  TARGET_AR: "llvm-lib",
  TARGET_CC: "clang-cl",
  TARGET_CXX: "clang-cl",
  CARGO_ENCODED_RUSTFLAGS: [
    "-Clinker-flavor=lld-link",
    `-Lnative=${crtLib}`,
    `-Lnative=${sdkUmLib}`,
    `-Lnative=${sdkUcrtLib}`,
  ].join(sep),
  BINDGEN_EXTRA_CLANG_ARGS_x86_64_pc_windows_msvc: includeArgs.map((path) => `-I${path}`).join(" "),
  CFLAGS_x86_64_pc_windows_msvc: cFlags,
  CXXFLAGS_x86_64_pc_windows_msvc: `${cFlags} /EHsc`,
  RCFLAGS: includeArgs.map((path) => `-I${path}`).join(" "),
  LIB: `${crtLib};${sdkUmLib};${sdkUcrtLib}`,
  PATH: `${llvmBinDir}:${cacheLinkRoot}:${process.env.PATH ?? ""}`,
};

console.log(`[windows-xwin] cache=${sourceCacheRoot}`);
console.log(`[windows-xwin] llvm=${llvmDir}`);
const result = spawnSync(
  "cargo",
  ["build", "--target", "x86_64-pc-windows-msvc", "--release", "--features", "tauri/custom-protocol"],
  {
    cwd: join(linkRoot, "src-tauri"),
    env,
    stdio: "inherit",
  },
);
rmSync(linkRoot, { recursive: true, force: true });
rmSync(cacheLinkRoot, { recursive: true, force: true });
process.exit(result.status ?? 1);
