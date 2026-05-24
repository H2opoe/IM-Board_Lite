# IM-Board Lite macOS 打包要求

## 版本号

- Lite macOS 应用包版本号：`2.1.4-lite`
- Lite macOS 系统版本号：`2.1.4`
- Lite macOS 发布展示版本号：`2.1.4-lite`
- 打包脚本、验包脚本、关于窗口展示统一通过 `app/scripts/release-config.mjs` 的 `releaseConfig()` 读取版本配置。
- npm、Cargo、Tauri 的静态清单仍必须保留版本字段：修改版本时必须同步 `app/package.json`、`app/package-lock.json`、`app/src-tauri/tauri.conf.json`、`app/src-tauri/Cargo.toml` 和 `app/src-tauri/Cargo.lock`。
- 平台不单独写入应用版本号，只写入最终产物文件名。

## Lite macOS 版

- 分支/目录：`codex/lite`，`/Volumes/SanDisk SSD Plus/Applications Data/Codex/IM-Board-lite`
- 应用包版本号：`2.1.4-lite`
- 应用系统版本号：`2.1.4`
- 发布展示版本号：`2.1.4-lite`
- 产物命名：`IM-Board_2.1.4-lite_mac_universal.dmg`
- 打包目标：`universal-apple-darwin`
- Lite 功能边界：不包含微信账号绑定和微信同步入口，历史微信账号只允许展示和删除，不允许启用、测试读取或同步。
- CLI 内置边界：Lite macOS 版不包含微信功能；飞书、钉钉、企微 CLI 通过应用内后台热更新获取。
- 运行时边界：非 macOS 自带的运行时依赖需要随 App 打包，不能依赖用户手动安装。
- 验证重点：确认 DMG 内不包含用户 profile/账号状态，确认没有微信 bridge、微信 CLI 或超出 Lite 边界的 CLI 资源。
- Lite Windows 版打包要求以 `/Volumes/SanDisk SSD Plus/Applications Data/Codex/IM-Board-lite-win/PACKAGING.md` 为准，避免 macOS Lite 目录误承接 Windows 产物。
- App 身份要求：`CFBundleIdentifier`、codesign Identifier 必须稳定为 `com.local.im-board`，`CFBundleName`/`CFBundleDisplayName` 必须为 `IM-Board`，`CFBundleShortVersionString` 必须使用不带 beta 后缀的系统版本号，`CFBundleVersion` 必须使用递增数字构建号。
- Beta 版规则：beta 字符串只允许出现在 `package.json` 的 `release.label`、关于窗口、更新日志和安装包文件名中，不能写入 macOS `.app` 的 Bundle ID、codesign Identifier、`CFBundleShortVersionString`。
- 发布前必须使用 `npm run package:macos` 或在 Tauri 打包后运行 `npm run repair:macos-app -- <IM-Board.app路径> --dmg <最终DMG路径>`，确保最终 `.app` 删除 `LSRequiresCarbon` 后重新 codesign，DMG 也来自修复后的 `.app`。
- 发布前必须对最终 `.app` 运行 `npm run verify:macos-app -- <IM-Board.app路径>`，确认 Info.plist 没有 `LSRequiresCarbon`、签名封口覆盖 Info.plist 和资源、LaunchServices 能按当前 bundle 重新注册。
