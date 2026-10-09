# xssc 文档

本文档描述 `1.0.0` 候选当前到未来版本的统一升级流程。正式目标为 Linux AMD64 GNU；工具没有常驻服务或前端。`support --json` 列出六个 Server 的统一机制支持，Foundation 是库。实际资源、结构和部署方式始终由产品当前校验与签名目标定义决定。

| 文档 | 读者任务 |
|---|---|
| [初学者指南](beginner-guide/README.md) | 理解信任、快照、维护锁、阶段和恢复 |
| [流程](project-workflow.md) | 阅读入口与状态机并定位实现 |
| [功能与取舍](feature-inventory-and-tradeoffs.md) | 核对实现、验证和限制 |
| [离线升级](offline-upgrades.md) | 制作制品、准备计划和执行恢复 |
| [Sunshine 观察缓存准备](sunshine-protocol-preparation.md) | 在受控停服中从 0.15.0 准备 0.16.0 的新会话契约 |
| [运维](operations.md) | 检查条件、演练、故障处理与发行 |
| [产品仓库](server-client-repositories.md) | 确认产品权威输入的归属 |
| [1.0.0 候选](releases/1.0.0.md) | 查看本轮范围和验证边界 |

Foundation `xcss-contracts`、`xcss-state-file`、`xcss-fs-safety`、`xcss-log`、`xcss-sqlite` 固定官方 Git source、精确 `=1.0.0` 与完整 revision `d58b9ef0822984ee0d29fb8b8139cfd2787374fb`。本工具拥有编排、签名/发行策略、文件快照及恢复状态机；产品拥有结构和业务约束。SQLite 用于当前产品测试及唯一的 Sunshine 0.15.0 → 0.16.0 离线观察缓存准备；它不提供任意 SQL hook 或运行时历史兼容。

Foundation 1.0.0 已正式发布，0.6.0 原生 CI 与正式发行已验证远端受控输入。1.0.0 的源码、原生 CI 与签名制品证据独立记录，不能继承上个版本的发行结论。

[原生边界审核](unsafe-audit.md)记录当前工具的必要 unsafe、已采用的安全替代与实际验证范围。

正式离线包保留仓库中的 `docs/` 相对结构；根 `README.md` 与 `OFFLINE-UPGRADES.md` 指向完整运维、升级和观察缓存准备说明。所有相对文档/资源引用与本地锚点在封装、签名前和解包后验证，源文档不为打包重复维护一套。
