# 事项业务日与数据库 v5

## 升级

`005_action_detected_day.sql` 为 `action_items` 增加可为空的 `detected_day`。完整版本支持旧本地 schema 0–4 升到 5；Lite 版本支持自身旧本地 schema 0–3 升到 5，其未使用的 v4 本地库仍须通过显式只读导入入口处理。高于 5 的库拒绝打开。

原 001 迁移和现有显示字段不变。正常启动先保存升级备份，再在一个事务中运行迁移和更新 `user_version`；迁移失败回滚。v5 库不能直接给旧应用打开，恢复旧版本应使用升级前备份。

Lite 的显式导入支持已知源 schema 1–5，按两库共同列复制。旧源无 detected_day 时保留 NULL，新源有该列时保留其值；未知版本保留原库并拒绝导入。账号允许范围和凭据隔离保持原策略。

## 写入与归属

新 AI 事项首次写入保存经过格式校验的分析 `day`。upsert 保留首次 `detected_day` 和现有 `first_detected_at`，不会用下一次分析日猜补旧 NULL 值。

读取归属顺序：

1. 可定位且日期合法的来源消息 `max(day)`，沿用事项获取新来源后的现有政策。
2. 合法 `detected_day`。
3. 两者缺失或非法时，解析旧时间的保守候选；无效来源日期不会遮盖合法的首次检测日。

旧时间混有来源消息本地 naive 和检测时刻 RFC3339，且没有历史清理设置和时区。候选解析保留自然日、可能的上一日、当前配置下的业务日；DST fold 分别检查两次瞬时，gap 和非法值不能猜补。只有全部候选都已过期才清理已完成或忽略事项。无法恢复的归属保留。open reply/task 按现有政策延续。

这保证新记录能准确换日；旧记录可能有意多保留，未知历史无法被完全恢复。显示时间继续遵循已有接口。

## 诊断 JSON

`dailySummary[].actionCount` 只统计来源消息或 detected_day 确认的日归属。旧歧义事项不再静默按 UTC 计入某天。

新增顶层 `actionDaySummary`：

- `schemaVersion: 1`
- `dailySummaryActionCountScope: "confirmed-day-only"`
- `profiles[]` 每个账号只出现一次，包含 `confirmedActionCount`、`uncertainActionCount`、`unassignedActionCount`

这些是全库按账号的事项数量；每日列表仍以存在消息的 day/profile/platform 分组并限制 80 行，不能把每日列表求和当作全库事项总量。未知数量不重复附到每个 day。这个兼容变化只影响诊断导出，Dashboard/Tauri 展示类型和已有显示字段不变。
