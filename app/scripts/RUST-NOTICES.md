# 离线 Rust 许可材料收集器

需要 Python 3.11 或更新版本。脚本只执行离线 Cargo tree、Cargo metadata 和工具链信息查询，不编译应用、不运行 build.rs、不启动业务运行时。

## 两个独立步骤

- `collect` 生成可复核的候选材料目录；缺口保留在 `manifest.json`，允许生成用于审查的结果。
- `verify` 重算当前输入并核对目录内所有文件，拒绝陈旧锁文件、上下文、源归档、展开源码、固定材料或输出。
- `verify --strict` 额外阻断缺少完整条款、补充材料中的版权缺口、SDK/MPL 等待审核事项、标准库材料缺失，以及跨操作系统的候选构建图。

退出码：0 表示该命令通过；1 表示输入、完整性或可重复性错误；2 表示严格材料门槛仍有缺口。`--strict` 只接受 `verify`。

从本版 `app` 目录运行，例如：

```sh
python3 scripts/rust-notices.py collect --manifest src-tauri/Cargo.toml --lock src-tauri/Cargo.lock --cargo-home /absolute/cargo-home --supplement shared/third-party/application/upstream/rust/materials.json --cargo /absolute/toolchain/bin/cargo --rustc /absolute/toolchain/bin/rustc --output /absolute/rust-notices-candidate --edition IM-Board --package im-board --host aarch64-apple-darwin --target aarch64-apple-darwin --profile release --feature-mode default
```

把 `collect` 改成 `verify`，保持全部输入参数一致即可复核；添加 `--strict` 执行严格检查。命令中的绝对路径须替换为真实工具链、缓存和独立输出路径。Windows 原生构建请使用实际 Windows 主机三元组和可执行文件路径。

## 参数契约

| 参数 | 含义 |
|---|---|
| `--manifest` / `--lock` | 当前 Cargo.toml 和实际 workspace Cargo.lock；禁止替换其他锁文件快照。 |
| `--cargo-home` | 已填充的离线 Cargo 缓存。每个实际候选 `.crate` 必须存在并吻合锁文件 checksum，展开源码也须逐文件一致。缓存标记不作为信任依据。 |
| `--cargo` / `--rustc` | 明确的工具链可执行文件；记录文件哈希、版本、commit 和 rustc sysroot。 |
| `--host` / `--target` | 明确实际主机和构建目标。host 必须吻合 cargo/rustc 报告；同时查询 target 与 host metadata 以核对主机构建依赖。 |
| `--feature-mode` | 必填 `default`、`no-default` 或 `all`，分别对应默认 features、`--no-default-features`、`--all-features`。 |
| `--features` | 可选逗号分隔的实际 features，必须与真实构建选择一致。 |
| `--profile` | 明确构建意图，如 `release`；Cargo tree 无法证明最终链接产物或 profile。 |
| `--edition` / `--package` | edition 必须吻合固定材料清单；package 为本版 Cargo.toml 的 package.name。 |
| `--supplement` | 正常源码中的固定补充材料清单，见下表。全部文件均验证 URL、固定 revision、checksum、SHA 和目录清单。 |
| `--output` | 独立候选目录；不得与 Cargo 源码、缓存或补充材料目录重叠。已有非本工具目录不会被覆盖。 |

| 版本 `--edition` | 常用 `--target` | 固定补充清单 |
|---|---|---|
| `IM-Board` | `aarch64-apple-darwin` / `x86_64-apple-darwin` | `shared/third-party/application/upstream/rust/materials.json` |
| `IM-Board-win` | `x86_64-pc-windows-msvc` | 同上 |
| `IM-Board-lite` | `aarch64-apple-darwin` / `x86_64-apple-darwin` | 同上 |
| `IM-Board-lite-win` | `x86_64-pc-windows-msvc` | 同上 |
| `IM-Board-demo` | `aarch64-apple-darwin` / `x86_64-apple-darwin` | `scripts/application-license-sources/rust/materials.json` |

Demo 请将示例参数替换为该版 edition、实际 package.name 和专属补充清单。脚本、测试及这些静态构建材料不会加入 Demo 的桌面运行功能。

## 范围和限制

候选组件来自实际 `cargo tree --edges normal,build`。metadata 只映射身份，不能通过遍历 metadata 替代候选图。normal 树仍包含 proc-macro 及其依赖；输出分别标明 normal 候选、仅 build 候选和 proc-macro 输入。

候选材料目录包含锁定归档及固定上游的许可/版权原文、精确来源和哈希、Cargo 查询证据、完整输入输出指纹，以及当前 rustc sysroot 的 `COPYRIGHT-library.html`。标准库文档与应用锁文件组件分别登记。许可声明、完整原文、通用条款模板和实际版权分别记录；AUTHORS 元数据和模板占位符不会代替组件版权。

当前版本只支持没有身份歧义的默认 crates.io 组件。git、path、自定义 registry、未知许可变体或例外必须增加明确支持和回归证据，不能猜测。已认证全文采用保守的结构识别；不认识的文本仍保留原文并形成缺口。

只有精确 Cargo 状态行 `Blocking waiting for file lock on package cache` 会写入执行日志并从可重复性指纹中剔除；其他 stderr 全部保留和复核。构建配置环境变量仅保存 SHA，避免输出原始值。

2026-09-30 验证仍有真实缺口：objc2 系列、convert_case/mac 的完整上游许可/版权；selectors 的 MPL 源码义务与 Apple SDK 相关审核。Brotli 的固定版本 BSD 原文已补齐，仍须同时保留其 MIT 原文；两个 SipHasher 版本使用锁定版权/声明中的明确官方链接，分别保存实际版权和 Apache 全文并选择 Apache 分支。当前 macOS arm64 候选的 main/Lite 阻断组件为 14，Demo 为 12；Windows 的 macOS host 候选为 3，并另有跨平台上下文阻断。精确清单以本次生成的 `manifest.json.strictIssues` 为准。macOS 上查询 Windows 目标仍包含 macOS host 依赖，不能用于声称原生 Windows 发布材料完备。

候选图与许可材料通过不证明 binary SBOM、真实链接/安装包包含项、系统 SDK 权利或发行合规已经完成。Universal 的两个目标和原生 Windows 必须分别验证实际发行上下文，再核验最终产物。

输出先写入独立 staging，检查全部文件与输入，随后替换目录；可捕获异常会恢复旧目录。当前使用可移植的两次目录 rename：其中有短暂路径缺失窗口，硬退出/断电没有自动恢复 journal，可能留下锁和 backup/stage。遇到此情况保留残留目录，确认没有运行中的写入进程，人工校验旧目录后恢复；不要删除来源不明的目录或把残留目录直接作为已验收发行材料。

## 隔离回归

```sh
python3 scripts/test_rust_notices.py
```

测试使用临时目录、替身 Cargo/rustc、合成归档及自带的已认证许可原文；不读取真实账号、数据库或凭据，也不依赖 artifacts 目录。测试检查身份/锁/host/features、归档完整性和安全路径、缺口保留、标准库范围、全目录篡改、并发、失败回滚和发布前输入变化。
