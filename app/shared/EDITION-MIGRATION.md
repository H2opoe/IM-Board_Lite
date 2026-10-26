# Lite 版本与数据隔离

Lite 使用独立应用标识 `com.local.im-board-lite`、名称 `IM-Board Lite`、数据目录 `IMBoardLite` 和 AI 凭据空间 `com.local.im-board-lite.ai`。

从 3.0.1 起不再提供旧版目录数据导入。新账号通过平台管理绑定；软件不会因此访问、搬移或删除旧目录 `IMBoard` 中的数据、备份及配置。

当前 Lite 数据库仍按既有版本化迁移升级，升级前生成一致性备份。该流程仅处理当前 Lite 数据目录，不迁移其他版本的数据。事项业务日规则见 [ACTION-DAY-SCHEMA.md](ACTION-DAY-SCHEMA.md)。
