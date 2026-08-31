# Sarmg Upgrade 文档总览

本文档集描述当前 `0.2.0` 二进制。`support --json` 是能力的唯一权威；catalog 表示产品状态资源，不
代表每个产品一定存在某项命令。正式工具唯一目标为 Linux AMD64 GNU `x86_64-unknown-linux-gnu`；本仓库
是无常驻进程、无前端的离线 CLI，因此没有 React/Vite 客户端，也不创建空 `config/`/`deploy/` 目录。

| 分类 | 文档 | 内容 |
|---|---|---|
| 初学者学习指南 | [beginner-guide/README.md](beginner-guide/README.md) | generation、manifest、当前 adapter、journal 和锁基础 |
| 工作流程与流程树 | [project-workflow.md](project-workflow.md) | 当前备份/恢复、无历史 edge 边界和 crash recovery 流程 |
| 完整功能与取舍 | [feature-inventory-and-tradeoffs.md](feature-inventory-and-tradeoffs.md) | 实现台账、产品矩阵、限制与安全取舍 |
| 必要 README | [../README.md](../README.md) | 项目边界、质量门和文档入口 |
| 运维 | [operations.md](operations.md) | 命令、停机、权限、备份、恢复、演练和发行 |

共享协议与产品工具的边界见[功能清单 2.1 节](feature-inventory-and-tradeoffs.md#21-foundation-与本工具的责任边界)：
Foundation 的 `sarmg-contracts`、`sarmg-schema-identity` 均以 `=0.3.0` 和不可变 Git rev
`1fe326081cfd896f05ff502e80f99504797c14c6` 提供 driver-independent 当前线类型和 schema 算法；本仓库
拥有 rusqlite、产品版本、文件系统、密钥与恢复状态机。双方都不保留旧版本 fallback，开发与发行也不得
改用 workspace sibling、Cargo path dependency、可变 branch 或本地副本。
