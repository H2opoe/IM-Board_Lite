# IM-Board Lite Windows 打包要求

## 版本与身份

版本以 `app/release-manifest.json` 为来源，通过 `app/scripts/release-config.mjs` 的 `releaseConfig()` 读取。当前应用系统版本为 3.0.1，发布标签为 3.0.1-lite。

修改版本时同步 npm、Cargo、Tauri 静态清单，并运行 `npm run verify:release-config`。`releaseLabel` 用于展示和产物名；系统版本不含 Lite 或 beta 后缀。Bundle ID 为 `com.local.im-board-lite`，产品名为 `IM-Board Lite`。macOS 构建号由 `macosBundleVersion` 配置，必须递增。

工作树分支：`codex/lite-win`。在该工作树的 `app` 目录执行以下命令。

## 构建及验包

```sh
npm run debug:env:windows
npm run tauri:build:windows
npm run package:windows:portable
```

原生目标为 `x86_64-pc-windows-msvc`，需 Visual Studio C++ 工具及 Windows SDK。开发运行使用 `npm run tauri:dev:windows`。macOS 交叉构建使用 `npm run tauri:build:windows:xwin`，需匹配的 LLVM、Cargo xwin 和 MSVC SDK；构建包含 `tauri/custom-protocol`，最终程序使用已构建网页。

便携包读取实际 CARGO_TARGET_DIR（相对目录以 app/src-tauri 为基准）及已验收的对应版本模板。原始 Cargo 程序名可为 im-board.exe，包内程序名由当前版本 wrapper 决定。

打包使用相邻独占目录；完整资源清单、ZIP 元数据、解压后的大小／CRC／SHA 及版本边界均通过后才替换旧 ZIP。失败只清理本次目录，保留既有归档及其他构建目录。Unix 使用 zip/unzip；Windows 使用系统 PowerShell .NET 压缩接口，工具失败会终止。

交叉目标 check 只证明编译检查。最终发行仍需原生启动、文件占用、账号替身、退出清理、安装升级及真实包内资源验收。

## 资源与功能边界

Lite 保留飞书、钉钉和企业微信；不得包含个人微信源码、绑定／同步、CLI 或专用 Python 运行时。运行 `npm run verify:lite-boundary`，并核对最终安装包的内容。

当前 Tauri 资源清单包含对应平台的 llama.cpp、DWS 及许可材料；完整版另含个人平台资源。运行时下载固定大小和 SHA-256，镜像必须提供相同字节。飞书／企业微信的官方 CLI 安装更新由各自运行时管理器负责。源码边界、依赖和资源清单检查仍须与真实发行包内容共同核验。

Demo 仅保留桌面壳、网页展示、主题与示例数据，资源清单为空，不能按正式后端版本同步。最终包不得携带用户账号、聊天记录、凭据、调试缓存或跨平台载荷。
