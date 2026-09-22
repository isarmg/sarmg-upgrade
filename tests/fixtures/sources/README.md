# Current source fixtures

这些夹具只包含专用测试凭据和虚构业务数据。每个 `database.sql` 是对应产品当前版本的精确 DDL，
`seed.sql` 固定一个管理员、一个有效 Session、Unicode/长度边界业务数据和审计证据。Dufs 的管理员与
Session 属于静态配置/内存状态，因此额外以 `auth.yaml` 和行为 Golden JSON 表达，重启后明确失效。

本目录及 `tests/source_fixtures.rs` 是具体产品状态、脱敏 source fixture 和迁移支持关系的唯一所有者。
Foundation 只定义产品中立的 Schema 身份与备份合同，不知道本目录布局，也不验证某个产品的历史状态。

夹具只用于当前合同校验；不保留已退役版本的 parser、升级边或兼容夹具。新增受支持来源时，应新建版本
目录并在 Upgrade 测试中固定产品、来源版本和 Schema fingerprint，实际重建 SQLite 后检查身份、代表性
管理员/Session/业务/审计记录及外键完整性。
