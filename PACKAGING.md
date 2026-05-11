# IM-Board Lite macOS 打包要求

## 版本号

- Lite macOS 应用版本号：`2.1.3-lite`
- 平台不写入应用版本号，只写入最终产物文件名。
- 修改版本时必须同步 `app/package.json`、`app/package-lock.json`、`app/src-tauri/tauri.conf.json`、`app/src-tauri/Cargo.toml` 和 `app/src-tauri/Cargo.lock`。

## Lite macOS 版

- 分支/目录：`codex/lite`，`/Volumes/SanDisk SSD Plus/Applications Data/Codex/IM-Board-lite`
- 版本号：`2.1.3-lite`
- 产物命名：`IM-Board_2.1.3-lite_mac_universal.dmg`
- 打包目标：`universal-apple-darwin`
- Lite 功能边界：不包含微信账号绑定和微信同步入口，历史微信账号只允许展示和删除，不允许启用、测试读取或同步。
- CLI 内置边界：Lite macOS 版不包含微信功能；飞书、钉钉、企微 CLI 通过应用内后台热更新获取。
- 运行时边界：非 macOS 自带的运行时依赖需要随 App 打包，不能依赖用户手动安装。
- 验证重点：确认 DMG 内不包含用户 profile/账号状态，确认没有微信 bridge、微信 CLI 或超出 Lite 边界的 CLI 资源。
- Lite Windows 版打包要求以 `/Volumes/SanDisk SSD Plus/Applications Data/Codex/IM-Board-lite-win/PACKAGING.md` 为准，避免 macOS Lite 目录误承接 Windows 产物。
