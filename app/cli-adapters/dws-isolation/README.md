# DWS 固定源码与平台适配

基于官方 1.0.65 的固定提交与源码归档 SHA-256，变更保存在 isolation.patch；上游 Apache-2.0 许可证随附。source-lock.json 记录上游提交、归档、补丁哈希和工具链版本。

补丁将 Windows keychain 的全部槽位放入 `Software\IMBoard\DwsCli\Profiles\<namespace>\<service>`，保留上游 DPAPI 加密。环境变量 `IM_BOARD_DWS_NAMESPACE` 必须是明确的 64 位十六进制摘要，缺失或无效时拒绝读写，不回退到全局 DWS keychain。摘要由宿主的账号隔离身份生成；普通 token、企业 token 和其他 secret 槽位使用同一隔离根。

`--im-board-isolation-version` 返回固定协议标记，用于宿主验证安装物支持该隔离协议；仍需同时验证产物哈希。

## 读取与原生组件

应用使用 `chat +search-msg` 直接按带时区的时间范围搜索消息，不先枚举联系人；`chat +recent-conversations` 可直接发现活跃单聊和群聊。投影补丁保留会话名称与 singleChat 标记。读取必须检查 complete/hasMore/failures/解密失败账本，不能把不完整结果当作空结果成功。适配入口禁用上游遥测。

1.0.65 的 SafeChat 解密组件要求 **CGO_ENABLED=1**。CGO=0 会编入不可用的 stub，不可作为本适配的完整发行物。macOS 使用系统 clang，Windows 原生构建需 Go 可用的 C/C++ 编译器；从 macOS 交叉编译 Windows 时须显式设置 `IMBOARD_DWS_WINDOWS_CC` 和可选 `IMBOARD_DWS_WINDOWS_CXX`（本轮使用经官方摘要校验的 Zig 0.17.0，target 为 x86_64-windows-gnu）。

上游随固定快照携带的 AEM/SafeChat SDK 未提供完整许可及再分发授权证据。默认构建不会将缺少许可证的 SafeChat 依赖当作已验证。**仅本地测试**可显式使用 `--allow-unverified-vendor-licenses` 生成带未验证声明的依赖清单；该参数不代表取得再分发许可，正式发布仍需补齐证据。

## 可复现构建

准备 source-lock.json 指定的 Go 工具链，在 app 目录执行：

```sh
python scripts/build-dws-isolated.py --work-dir /path/to/new-build-directory --output /path/to/dws-windows-x64.exe --allow-unverified-vendor-licenses
```

可用 `--source-archive /path/to/source.tar.gz` 提供预下载源码，仍必须通过相同的大小及固定 SHA-256 校验。构建目录必须新建或为空。脚本验证源码及补丁哈希，使用锁定的 go.sum，执行无凭据的命名空间单元测试，并生成 Windows 测试程序及 CLI。Windows 主机额外执行仅使用随机隔离命名空间的 DPAPI／注册表测试。

宿主安装器及运行入口已接入固定哈希产物，绑定与读取统一传入账号命名空间。后台读取会校验并迁移应用保存的旧 DPAPI token，不访问全局授权槽位；新绑定直接写入账号命名空间。旧全局 token 交换逻辑已删除。CI 会在 Windows 重建产物并执行隔离测试；本地交叉编译不代表这些原生测试已通过。账号删除已接入持久化凭据清理队列：按命名空间重新检查其他账号／草稿引用，与会话共用互斥锁，失败保留重试记录；清除凭据后保留不含秘密的删除标记，拒绝延迟到达的旧会话。新增 Windows 原生清理、忙碌重试及隔离测试仍待 CI 执行。强制结束宿主后实际 DWS 子进程是否存活并继续写入，仍需原生验证。

安全依赖适配：WebSocket 固定为 1.5.3，x/sys 为 0.47.0、x/text 为 0.41.0。PBKDF2 直接使用 Go 标准库，移除仅用于包装该函数的 x/crypto 依赖；独立兼容向量及加解密测试纳入构建。完整模块清单及可获得的许可文本随运行时交付。go-localereader 0.0.1 只有 README 中的 MIT 声明，清单明确标注这一证据缺口。CI 使用固定 govulncheck 版本扫描实际产物，并重建比对二进制及依赖证据。

## macOS 安全修复包

main 与 MacLite 使用同一固定上游源码与依赖安全补丁；macOS 原有账号 keychain 行为保持不变，Windows 注册表补丁由 Go 构建标签排除。运行时以每种架构独立的 package.tgz 随应用携带，安装时校验归档和解包后二进制，旧版或篡改文件不能作为有效 DWS 执行。归档形式保留载荷字节，实际签名包仍需验收。

```sh
python scripts/build-dws-isolated.py --target macos --work-dir /path/to/new-build-directory --output /path/to/archive-output --allow-unverified-vendor-licenses
```

输出目录包含两种架构的归档及 manifest.json。Windows 仍使用默认 `--target windows`，`--output` 指定 exe 文件。构建使用临时目录中的合成密钥测试，不运行真实账号授权。
