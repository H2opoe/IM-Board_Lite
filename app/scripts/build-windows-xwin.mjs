import { releaseBuildEnv } from "./release-build-env.mjs";
import { existsSync, mkdtempSync, rmSync, statSync, symlinkSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
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
  join(llvmBinDir, "llvm-rc"),
  join(sourceCacheRoot, "xwin", "crt", "include", "vcruntime.h"),
  join(sourceCacheRoot, "xwin", "sdk", "include", "ucrt", "assert.h"),
  join(sourceCacheRoot, "xwin", "crt", "lib", "x86_64", "vcruntime.lib"),
  join(sourceCacheRoot, "xwin", "sdk", "lib", "um", "x86_64", "kernel32.Lib"),
  join(sourceCacheRoot, "xwin", "sdk", "lib", "ucrt", "x86_64", "ucrt.lib"),
];

function fail(message) {
  console.error(`[windows-xwin] ${message}`);
  process.exit(1);
}

for (const path of requiredPaths) {
  if (!existsSync(path) || !statSync(path).isFile()) {
    fail(`cargo-xwin 缓存不完整：${path}`);
  }
}

// cc-rs 默认按空白拆分 CFLAGS；短链接目录本身也必须没有空白或 PATH 分隔符。
if (process.platform === "win32") fail("此交叉编译入口需要 Unix 主机；Windows 请使用 tauri:build:windows。");
const temporaryRoot = /[\s:]/.test(tmpdir()) ? "/tmp" : tmpdir();
// 独占目录隔离同一工作树的并发构建，只清理本次创建的目录。
const workspaceRoot = mkdtempSync(join(temporaryRoot, "im-board-xwin-"));
const linkRoot = join(workspaceRoot, "app");
const cacheLinkRoot = join(workspaceRoot, "cache");
try {
  symlinkSync(appRoot, linkRoot, "dir");
  // 外置盘路径包含空格；C 编译器参数通过临时符号链接引用真实缓存，避免 cc-rs 拆参失败。
  symlinkSync(sourceCacheRoot, cacheLinkRoot, "dir");

  const sep = "\u001f";
  // 编码后的 Rust 参数支持空格，保留原缓存路径以复用纯 Rust 构建指纹。
  const crtLib = join(sourceCacheRoot, "xwin", "crt", "lib", "x86_64");
  const sdkUmLib = join(sourceCacheRoot, "xwin", "sdk", "lib", "um", "x86_64");
  const sdkUcrtLib = join(sourceCacheRoot, "xwin", "sdk", "lib", "ucrt", "x86_64");
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

  const env = releaseBuildEnv(appRoot, {
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
  }, [linkRoot]);

  console.log(`[windows-xwin] cache=${sourceCacheRoot}`);
  console.log(`[windows-xwin] llvm=${llvmDir}`);
  const result = spawnSync(
    "cargo",
    ["build", "--locked", "--target", "x86_64-pc-windows-msvc", "--release", "--features", "tauri/custom-protocol"],
    {
      cwd: join(linkRoot, "src-tauri"),
      env,
      stdio: "inherit",
    },
  );
  if (result.error) console.error(`[windows-xwin] cargo 无法启动：${result.error.message}`);
  process.exitCode = result.status ?? 1;
} finally {
  rmSync(workspaceRoot, { recursive: true, force: true });
}
