# xssc 文档

本文档描述 `1.0.0` 候选当前到未来版本的统一升级流程。正式目标为 Linux AMD64 GNU；工具没有常驻服务或前端。`support --json` 列出六个 Server 的统一机制支持。实际资源、结构和部署方式始终由产品当前校验与签名目标定义决定。

| 文档 | 读者任务 |
|---|---|
| [Linux 部署与维护](platform-setup.md) | 安装依赖、下载和验签、安装工具、诊断、更新与卸载；确认无需配对和无常驻服务 |
| [初学者指南](beginner-guide/README.md) | 理解信任、快照、维护锁、阶段和恢复 |
| [流程](project-workflow.md) | 阅读入口与状态机并定位实现 |
| [功能与取舍](feature-inventory-and-tradeoffs.md) | 核对实现、验证和限制 |
| [离线升级](offline-upgrades.md) | 制作制品、准备计划和执行恢复 |
| [运维](operations.md) | 检查条件、演练、故障处理与发行 |
| [产品仓库](server-client-repositories.md) | 确认产品权威输入的归属 |
| [1.0.0 候选](releases/1.0.0.md) | 查看本轮范围和验证边界 |

本工具只依赖一个 `xcsc` 包，启用 `offline-maintenance` feature；`xcsc::contracts`、`xcsc::state_file`、`xcsc::fs_safety`、`xcsc::log` 和 `xcsc::sqlite` 均为内部模块。该包固定官方 Git source、精确 `=1.0.0` 与完整 revision `c4ad7383d079efb5903872ddb66a8386ef406121`。本工具拥有编排、签名/发行策略、文件快照及恢复状态机；产品拥有结构和业务约束。SQLite 用于当前状态的有界只读验证和产品测试；它不提供任意 SQL hook 或运行时历史兼容。

当前共享输入是 xcsc 单体；历史 0.6.0 原生 CI 与正式发行记录仅证明其当时输入。1.0.0 的源码、原生 CI 与签名制品证据独立记录，不能继承上个版本的发行结论。

[原生边界审核](unsafe-audit.md)记录当前工具的必要 unsafe、已采用的安全替代与实际验证范围。

正式离线包保留仓库中的 `docs/` 相对结构；根 `README.md` 与 `OFFLINE-UPGRADES.md` 指向完整运维、升级和部署说明。所有相对文档/资源引用与本地锚点在封装、签名前和解包后验证，源文档不为打包重复维护一套。

公共支撑的职责、单体依赖、平台边界与验证方法见[公共支撑说明](common-support.md)。
