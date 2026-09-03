# Current source fixtures

这些夹具只包含专用测试凭据和虚构业务数据。每个 `database.sql` 是对应产品当前版本的精确 DDL，
`seed.sql` 固定一个管理员、一个有效 Session、Unicode/长度边界业务数据和审计证据。Dufs 的管理员与
Session 属于静态配置/内存状态，因此额外以 `auth.yaml` 和行为 Golden JSON 表达，重启后明确失效。

夹具只用于当前合同校验；不保留已退役版本的 parser、升级边或兼容夹具。
