# 10. 源码路线、演练与术语表

## 10.1 阅读路线

先读 CLI/support/catalog/current identity，再读 manifest/path/locking/SQLite clone，随后读 restore/journal，
最后逐产品 adapter 与 release scripts。不要从某个命令示例反推通用安全模型。

## 10.2 按问题找入口

| 问题 | 入口 |
|---|---|
| 命令是否支持 | support/catalog/CLI |
| source 被拒绝 | current identity、adapter validator |
| backup 失败 | snapshot、inventory、manifest、fsync |
| restore 中断 | journal、stage、recover command |
| tree 超限 | budgets、inventory/walker |
| key 失败 | credential requirement/envelope verifier |
| release 验证 | release metadata、checksum/signature scripts |

## 10.3 最小演练集

1. current backup/verify 与单字节篡改拒绝。
2. no-clobber output/target。
3. WAL 有提交页的 SQLite snapshot。
4. symlink/hardlink/special file/path traversal 负例。
5. restore 每阶段中断的 commit/rollback。
6. external key 正确/错误/权限不安全。
7. 组合 tree 的 mode/hardlink/sparse/xattr 和 budgets。
8. target 产品 offline doctor 与启动 smoke。

## 10.4 术语

| 术语 | 含义 |
|---|---|
| generation | 逻辑一致的一代数据库及相关资源 |
| adapter | 一个精确 source/target 或 current 合同实现 |
| edge | 精确历史版本之间的有向转换 |
| manifest | 备份资源、身份、Hash 与要求的严格合同 |
| code allowlist | 编译进 binary 的受支持身份集合 |
| sidecar | SQLite WAL/SHM/journal 等伴随文件 |
| stage | 同文件系统、验证后才安装的私有来件 |
| no-clobber | 目标存在就拒绝，不覆盖 |
| recovery journal | 记录持久切换阶段和原件/来件身份的证据 |
| preserved original | 切换时保全的原始状态代 |
| external key | 不进入备份、用于认证/解密持久 Secret 的密钥 |
| tree inventory | 对目录每项语义和聚合身份的完整描述 |
| fsync | 要求文件/目录变更进入持久存储边界 |

## 10.5 学成标准

能在不看命令示例时画出 backup/restore 时序；能解释每个 fsync/journal 的原因；能判断何时必须停止而非
猜 adapter；能独立演练中断恢复；能证明 raw key 未进入任何输出。

## 10.6 深入文档

完整端到端阶段见[工作流程](../project-workflow.md)，产品/edge/能力矩阵见[功能与取舍](../feature-inventory-and-tradeoffs.md)，
生产命令、保管和事件处置见[运维文档](../operations.md)。
