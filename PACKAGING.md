# IM-Board Lite macOS 打包要求

## 版本与身份

版本以 `app/release-manifest.json` 为来源，通过 `app/scripts/release-config.mjs` 的 `releaseConfig()` 读取。当前应用系统版本为 3.0.0，发布标签为 3.0.0-lite；这是 2026-09-30 的配置快照。

修改版本时同步 npm、Cargo、Tauri 静态清单，并运行 `npm run verify:release-config`。`releaseLabel` 用于展示和产物名；系统版本不含 Lite 或 beta 后缀。Bundle ID 为 `com.local.im-board-lite`，产品名为 `IM-Board Lite`。macOS 构建号由 `macosBundleVersion` 配置，必须递增。

工作树分支：`codex/lite`。在该工作树的 `app` 目录执行以下命令。

## 构建及验包

```sh
npm run package:macos
```

目标为 `universal-apple-darwin`。最终 DMG 路径及名称由 releaseConfig 计算；此命令先构建，再修复并验证 App，最后发布由修复后 App 制作的 DMG。

单独修复使用 `npm run repair:macos-app -- "<App路径>" --dmg "<最终DMG路径>"`，验证使用 `npm run verify:macos-app -- "<App路径>"`。默认 App 路径使用当前产品名。

修复先复制到相邻独占目录，删除 LSRequiresCarbon、更新身份后只重签外层，保留内部签名、权限及执行模式；`--deep` 只用于签名验证。候选 App 和 DMG 均验证通过后才替换旧产物；普通失败保留旧产物，恢复失败会保留并报告备份。强制终止／断电的恢复不属于普通回滚承诺。

最终发行还需核验 Developer ID、公证、Gatekeeper 和安装后的实际功能。脚本的 ad-hoc 签名或 spctl 警告不代表发行验收通过。

## 资源与功能边界

Lite 保留飞书、钉钉和企业微信；不得包含个人微信源码、绑定／同步、CLI 或专用 Python 运行时。运行 `npm run verify:lite-boundary`，并核对最终安装包的内容。

当前 Tauri 资源清单包含对应平台的 llama.cpp、DWS 及许可材料；完整版另含个人平台资源。运行时下载固定大小和 SHA-256，镜像必须提供相同字节。飞书／企业微信的官方 CLI 安装更新由各自运行时管理器负责。源码边界、依赖和资源清单检查仍须与真实发行包内容共同核验。

Demo 仅保留桌面壳、网页展示、主题与示例数据，资源清单为空，不能按正式后端版本同步。最终包不得携带用户账号、聊天记录、凭据、调试缓存或跨平台载荷。
