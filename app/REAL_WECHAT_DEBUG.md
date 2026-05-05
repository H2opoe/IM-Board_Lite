# 真实微信调试环境

## 当前链路

```text
Tauri App
  -> Rust bridge_runner
  -> 语义命令 list-chats / fetch-messages
  -> app/bridges/wechat/bridge_main 映射到实际 CLI 命令
  -> 本机 wechat_cli 源码 init / 全局 wechat-cli 查询
  -> Profile 独立 config.json、all_keys.json、cache
```

## CLI 命令映射

同步主流程不直接依赖某个平台 CLI 的命令名，只向 bridge 发送：

```text
list-chats       拉取今日相关会话列表
fetch-messages   拉取指定会话消息
```

微信 bridge 默认映射为：

```json
{
  "cliCommands": {
    "listChats": "sessions",
    "fetchMessages": "history"
  },
  "cliArgPlacement": {
    "fetchMessages": {
      "chat": "positional"
    }
  }
}
```

如果后续平台 CLI 说明使用不同命令名或参数名，优先在对应平台 bridge 或 Profile `configJson` 的 `cliCommands`、`cliArgs`、`cliArgPlacement` 中适配，不要把平台私有命令名写进同步主流程。

## 启动前检查

```bash
cd app
npm run debug:env
npm run debug:wechat:discover
```

`debug:wechat:discover` 只扫描运行中的微信进程和数据目录，不读取聊天内容。

## 真实 App 启动

需要先安装 Rust 工具链，使 `cargo` 可用，然后启动：

```bash
cd app
npm run tauri:dev:real
```

启动后进入“平台管理”，点击“微信”，选择检测到的实例，输入本机 sudo 密码执行一次性初始化。密码只通过 stdin 传给本地 Bridge，不写入 Profile、不进入日志。

## 数据落点

每个 Profile 独立保存：

```text
~/Library/Application Support/IMBoard/Profiles/<profile_id>/config.json
~/Library/Application Support/IMBoard/Profiles/<profile_id>/all_keys.json
~/Library/Caches/IMBoard/<profile_id>/
```

Tauri 调试和 Release 包统一使用 `IMBoard`，方便调试时复用真实应用状态。

## 注意

- 读取真实聊天数据前，确认系统设置里运行 App 的终端或 IDE 已开启“完全磁盘访问权限”。
- 如果 init 报 `task_for_pid failed`，需要按 wechat-cli 的提示处理微信签名和重新启动微信。
- Bridge 会顺序执行 wechat-cli，不要并行跑多个微信读取命令。
