# IM-Board Lite 打包要求

## 版本号

- Lite macOS 应用版本号：`2.1.2-lite`
- Lite Windows 应用版本号：`2.1.2-lite`
- 平台不写入应用版本号，只写入最终产物文件名。
- 修改版本时必须同步 `app/package.json`、`app/package-lock.json`、`app/src-tauri/tauri.conf.json`、`app/src-tauri/Cargo.toml` 和 `app/src-tauri/Cargo.lock`。

## Lite macOS

- 分支/目录：`codex/lite`，`/Volumes/SanDisk SSD Plus/Applications Data/Codex/IM-Board-lite`
- 版本号：`2.1.2-lite`
- 产物命名：`IM-Board_2.1.2-lite_mac_universal.dmg`
- 打包目标：`universal-apple-darwin`
- CLI 内置边界：Lite 版不随包内置官方 CLI，平台 CLI 通过应用内后台热更新获取；Lite 功能边界以 README 的公开说明为准。
- 运行时边界：非 macOS 自带的运行时依赖需要随 App 打包，不能依赖用户手动安装。
- 验证重点：确认 DMG 内不包含用户 profile/账号状态，确认没有超出 Lite 边界的 CLI 或 bridge 资源。

## Lite Windows

- 分支/目录：`codex/lite`，`/Volumes/SanDisk SSD Plus/Applications Data/Codex/IM-Board-lite`
- 版本号：`2.1.2-lite`
- 产物命名：`IM-Board_2.1.2-lite_windows_x64_portable.zip`
- 打包目标：`x86_64-pc-windows-msvc`
- CLI 内置边界：Lite Windows 版官方 CLI 全部通过应用内后台热更新获取，不随安装包内置。
- 运行时边界：非 Windows 自带的运行时依赖需要随包提供，不能依赖用户手动安装。
- 验证重点：使用 `zip -X -r` 重新封装，确认没有 `__MACOSX`、`.app`、`.dylib`、`OfficialCli`、`bridges` 等 macOS 或旧 CLI 资源。
