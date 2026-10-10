# xssc 概念导读

首次实际使用，从[安装](../platform-setup.md)、[计划](../configuration.md)和[升级步骤](../offline-upgrades.md)开始。以下短文供按需了解设计，无需逐章读完才能使用工具。

## 理解一次升级

1. [项目概览](01-project-overview.md)
2. [环境与首次验证](02-safe-environment-and-first-validation.md)
3. [事务与中断恢复](05-restore-journal-and-crash-recovery.md)

## 按问题深入

- 快照为何包含数据库目录：[文件系统和持久性](03-filesystem-sqlite-and-durability-basics.md)
- 如何确定制品和保护范围：[签名与备份](04-capabilities-manifests-and-backup.md)、[产品校验](06-product-authority-and-protected-state.md)
- 当前支持哪些更新：[范围](07-current-contracts-and-scope.md)
- 如何修改和验证实现：[测试与接入](08-testing-debugging-and-product-integration.md)、[发行过程](09-release-security-and-operations.md)
- 查找代码与术语：[阅读路线](10-reading-roadmap-and-glossary.md)

完整字段和内部协议见[参考索引](../reference/README.md)。
