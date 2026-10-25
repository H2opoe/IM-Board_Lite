# 接口契约

Rust serde 模型是桌面接口的字段定义来源：

- `src-tauri/src/storage/models.rs`：ImProfile、SyncResult 与看板数据。
- `src-tauri/src/sync/job.rs`：SyncProgress，事件名 `sync-progress`。
- `src-tauri/src/bridge_runner/mod.rs`：BridgeEnvelope / BridgeError。成功响应的 `error` 为 `null`；业务数据位于 `data`，附加信息位于 `meta`。
- `src-tauri/src/connectors/mod.rs`：ConnectorCapabilityDescriptor。并发上限直接读取连接器执行配置。

账号设置放在 `configJson` 内；`authType`、`configPath`、`cacheDir` 不是账号顶层字段。

旧 `schemas/*.json` 无仓库内消费者，且与运行接口不符，已删除。实际运行接口未改名，字段未删除。本次不引入第二套手写 schema。`npm run test:scripts` 对照 Rust 字段和 TypeScript AST，检查账号、同步进度、同步结果和能力描述的字段及基础类型；Rust 回归验证序列化的成功与失败桥接响应。

新增或调整字段应同时更新前端模型和契约检查。平台枚举及 AI 状态在前端收窄，需分别遵守各版本的平台边界；测试不能替代真实连接器验证。
