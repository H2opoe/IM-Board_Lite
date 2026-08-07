# IM-Board Lite Windows 打包要求

## 版本号

- Windows Lite 版应用包版本号：`2.2.3-lite`
- Windows Lite 版系统版本号：`2.2.3`
- Windows Lite 版发布展示版本号：`2.2.3-lite`
- 打包脚本、验包脚本、关于窗口展示统一通过 `app/scripts/release-config.mjs` 的 `releaseConfig()` 读取版本配置。
- npm、Cargo、Tauri 的静态清单仍必须保留版本字段：修改版本时必须同步 `app/package.json`、`app/package-lock.json`、`app/src-tauri/tauri.conf.json`、`app/src-tauri/Cargo.toml` 和 `app/src-tauri/Cargo.lock`。
- 平台不单独写入应用版本号，只写入最终产物文件名。

## Windows Lite 版

- 分支/目录：`codex/lite-win`，`/Volumes/SanDisk SSD Plus/Applications Data/Codex/IM-Board-lite-win`
- 应用包版本号：`2.2.3-lite`
- 应用系统版本号：`2.2.3`
- 发布展示版本号：`2.2.3-lite`
- 产物命名：`IM-Board_2.2.3-lite_windows_x64_portable.zip`
- 打包目标：`x86_64-pc-windows-msvc`
- Lite 功能边界：不包含微信账号绑定、微信同步入口、微信 bridge 源码或微信专用运行时。
- CLI 内置边界：Windows Lite 版不得包含微信 CLI、独立 Python 运行时或 `third_party/wechat_bridge`；飞书、钉钉、企微 CLI 通过应用内后台热更新获取。
- 运行时边界：非 Windows 自带的运行时依赖需要随包提供，不能依赖用户手动安装。
- 手动使用 `cargo xwin` 交叉编译时必须带 `--features tauri/custom-protocol`，否则 Tauri 会按开发态编译，启动后访问 `127.0.0.1:1420` 的 Vite 服务。
- 验证重点：使用 `zip -X -r` 重新封装，并由打包脚本逐项扫描，确认没有 `__MACOSX`、`.app`、`.dylib`、微信 bridge、微信 CLI、微信专用 Python 运行时或超出 Lite 边界的 CLI 资源。
