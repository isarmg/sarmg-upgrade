# 06. 产品 Adapter 与组合状态

## 6.1 Adapter 不是通用脚本

每个 adapter 编译进精确产品、版本、Schema、资源和不变量。它不会根据表名相似或 manifest 自报选择逻辑。
缺失产品/版本意味着不支持，需要开发新 adapter。

## 6.2 Media Backup

当前状态是 SQLite + media data tree。备份、恢复和 recovery 使用 `backup-media`、`verify-media-backup`、
`restore-media`、`recover-media-restore` 专用命令；两者不可拆分，也不能使用 generic SQLite。

## 6.3 Host Monitoring

当前 Server 状态为 code-allowlisted SQLite，可使用受限 generic SQLite command。Agent 本地身份/Spool 不
自动包含在 Server backup。当前没有 Host 历史 edge。

## 6.4 Sunshine Manager

SQLite-only 物理资源还依赖 external credential key。verify/restore 必须提供精确 key ID/file并实际认证
所有 Host credential 和 unfinished operation；原始 key 不进包。

## 6.5 Sentinel Monitor

SQLite + MediaMTX config/contract + recordings tree + external key 是一个组合。当前工具尚未实现 Sentinel
组合命令，因此不能分别恢复数据库与录像，也不能使用 generic SQLite 冒充完整备份。

## 6.6 Dufs

SQLite + protected YAML + shared root 构成状态。当前工具尚未实现 Dufs 组合命令；未来实现必须处理目录
语义、budgets 以及 config/shared root 锁，当前不得使用 generic SQLite 替代。

## 6.7 Sarmg Foundation

Foundation 没有 runtime state；catalog 可说明这一事实，但不提供 backup/restore adapter。源码和 package
发布由 Git/registry 流程管理，不能伪装成数据库备份。

## 6.8 新名称边界

所有 adapter、manifest、CLI、测试和文档只使用当前产品名称。工具不接受另一 product slug，也不自动
重写旧 manifest 的名称。

## 6.9 选错命令

在 mutation 前通过 product、version、resource shape、Schema、external requirement 和 code allowlist 多重
拒绝。操作者不能用 generic command 绕过组合产品资源合同。
