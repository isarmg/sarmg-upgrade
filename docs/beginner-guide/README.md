# Sarmg Upgrade 初学者学习指南

这是高权限离线工具的十章教程。阅读顺序刻意先讲证据、状态代和锁，再讲命令；只会复制示例命令不足以
安全处理生产升级。下方单页速览保留，专题章节给出完整故障与恢复语义。

1. [项目定位、威胁模型与支持边界](01-project-overview.md)
2. [安全实验环境与第一次验证](02-safe-environment-and-first-validation.md)
3. [文件系统、SQLite 与持久性基础](03-filesystem-sqlite-and-durability-basics.md)
4. [能力目录、Manifest 与备份流程](04-capabilities-manifests-and-backup.md)
5. [恢复、Journal 与中断处置](05-restore-journal-and-crash-recovery.md)
6. [产品 Adapter 与组合状态](06-product-adapters-and-composite-state.md)
7. [为什么当前没有历史升级 Edge](07-historical-edges-and-current-contracts.md)
8. [测试、调试与新增 Adapter](08-testing-debugging-and-new-adapters.md)
9. [正式发行、安全与生产运维](09-release-security-and-operations.md)
10. [源码路线、演练与术语表](10-reading-roadmap-and-glossary.md)

以下内容是单页速览。

## 1. 为什么单独建立升级仓库

若每个产品运行时都携带历代数据库、配置、密文和路径兼容代码，新版本会不断扩大攻击面与测试矩阵。
Sarmg 的策略是：产品二进制只理解一个当前世界；备份、恢复和未来确有必要的历史转换由停机运行的
特权工具集中处理。当前仍在开发，尚无任何历史升级 adapter。

## 2. 五个核心概念

- **generation**：SQLite main 加当时存在的 WAL/journal，或数据库与数据树/配置/录像组成的一致状态代。
- **adapter**：对一个当前状态或未来精确 source/target 实现完整验证和处理的模块。
- **manifest**：备份中记录版本、Schema、文件、模式、Hash、资源和 external requirement 的严格 JSON。
- **maintenance lock**：与产品约定一致、用于阻止运行时和离线工具同时改变状态的锁。
- **recovery journal**：切换前持久记录原件、暂存件和阶段；中断后人工决定提交或回滚。

## 3. 先看能力，不猜命令

```bash
sarmg-upgrade support --json
sarmg-upgrade catalog --json
```

`support` 列出当前 binary 真正实现的 command/capability；自动化应读取它，而不是看产品是否出现在
catalog。Foundation 没有 runtime state，所以 catalog 中存在但没有 backup/restore adapter。

## 4. 当前实现范围

Media Backup `0.2.0`、Sentinel Monitor `0.2.0` 和 Dufs RAM `0.50.1` 支持严格组合备份；
Host Monitoring `0.8.0` 和 Sunshine Manager `0.8.0` 支持严格 SQLite-only 备份。所有历史升级 edge 均未实现。
自动化必须读取 `support --json`，不能根据 catalog 或本文推断命令。

## 5. 为什么先复制再解析

当前 SQLite 备份使用 SQLite online backup 取得逻辑一致快照，随后验证当前 Schema identity。未来历史
adapter 若加入，必须先在 exclusive maintenance lock 下保存 source 证据，再从零构建 target；这只是
准入规则，不是当前二进制能力。

## 6. 为什么不能只复制 app.db

WAL 模式下已提交数据可能仍在 `-wal`，只复制主文件会生成逻辑旧快照。Media Backup 还包括媒体树；
Sunshine 和 Sentinel 依赖 external credential key。Sentinel/Dufs 的组合 adapter 必须通过
`*-current` 命令同代处理数据库、数据树与精确配置集，不能用通用 `cp` 或 SQLite 命令替代。

## 7. 外部密钥

原始 key bytes 不写入 backup、manifest、journal 或 JSON 输出。manifest 只记录非秘密 key ID、算法、
envelope version/Hash 要求。备份验证和恢复时重新提供受保护 key，工具必须实际认证全部密文。

因此应在独立 Secret 管理系统中备份 key；只有数据备份没有 key，或只有 key 没有数据，都不能恢复。

## 8. 原子性与持久性

输出目录采用 create-new/no-clobber；manifest 最后写入并同步，出现完整 manifest 才表示 backup 完成。
恢复在目标同文件系统创建私有 stage，校验后记录 journal，再以原子 rename/交换逐步安装。每个关键阶段
同步目录。若进程在 mutation 后中断，保留 recovery directory，禁止手动拼接文件。

## 9. 第一次安全练习

只在临时 fixture 或备份副本上：

1. 运行 `support --json`。
2. 对已有 backup 运行相应 `verify-*`。
3. 用 `inspect-manifest` 查看严格 metadata。
4. 向一个全新临时目标 restore，不使用生产路径。
5. 停止/中断测试流程，观察 recovery 目录并分别演练 commit/rollback。
6. 校验恢复后产品 doctor，而不是仅检查命令 exit code。

## 10. 禁止事项

不要在服务运行时做排他恢复，不用 root 对用户可替换路径运行，不跟随 symlink，不覆盖已有 backup，
不编辑 manifest 绕过 Hash，不把 Secret 放到命令日志，不对未知版本手工迁移，不删除 recovery 目录后
重试，也不把通用 SQLite command 用于组合产品。

## 11. 学习前先固定的 current 事实

| 产品 | 当前 adapter | current identity | recover 边界 |
|---|---|---|---|
| Media Backup | SQLite + data tree 组合 adapter | `0.2.0` / r1 / `2563e6afc3fff272d02b7a5615272cc773862243bfd15aec51655abf1d9c6b1c` | 支持显式 commit/rollback |
| Host Monitoring | SQLite-only adapter | `0.8.0` / r1 / `12dd1e61426b6b99df3d429b8c36ee3a5b22d1da776d98fc960b45b4f58c8e05` | 支持显式 commit/rollback |
| Sunshine Manager | keyed SQLite-only adapter | `0.8.0` / r2 / `c9dedb33dd7a5ad613e762eb135a7aa5184ce1df52166459bee7b3485b4b3be3` | restore 可执行，但 recover 未对外支持 |
| Sentinel Monitor | DB/recordings/三配置/key 组合 adapter | `0.2.0` / r1 / `f547ddc817d830d23b5305bb1f88b29898d6531568edd6eb194c2b629eb560c0` | `recover-current` commit/rollback |
| Dufs RAM | DB/shared root/`dufs.yaml` 组合 adapter | `0.50.1` / r1 / `3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423` | `recover-current` commit/rollback |
| Sarmg Foundation | 无运行时状态 | 不适用 | 不适用 |

这些 SHA 是当前代码拥有的 allowlist，不是“同产品大致兼容”的版本提示。即使 manifest、metadata 与真实
数据库三者自洽，只要与 binary 内置值不同，current adapter 就必须拒绝。DDL 增删一个 index、trigger、
CHECK 或列定义也会改变 fingerprint；工具不会查看一个旧表名后进入兼容分支。

Media composite manifest 另有唯一 current wire version 3，不读取 v2。其 backup 根 exact 只有
`database.sqlite3`、`tree/`、`manifest.json`；tree inventory 将 root mode、非根目录 path/mode、文件
path/mode/size/SHA 纳入聚合摘要。root chmod、顶层 extra entry、hardlink、symlink 或 special file 都必须被
`verify-media-backup` 拒绝。

## 12. 三条信息源的优先级

```text
正在执行的 binary：support --json
  > 当前 checkout 的 CLI/help/代码
  > 本文和人工表格
  > catalog 资源描述
  > 文件名、目录名、manifest 自报值或操作者猜测
```

- `support` 说明 binary 真正编译进了哪些 current operation/version，并列出空的 `upgrade_edges`。
- `catalog` 说明一个产品完整状态理论上包含哪些资源。例如 Dufs 有 SQLite、data tree、configuration；
  这恰恰说明只备份 SQLite 不完整，而不是说明 generic SQLite adapter 可用。
- `inspect-manifest` 只证明 Foundation SQLite manifest 可严格解析，不证明文件 SHA、Schema、key 或可恢复性。
- `verify-*` 才读取并复核全部已实现资源；产品自身 offline doctor 和启动 smoke 又是工具验证之后的下一层。

## 13. 一次安全学习循环

1. 在 Linux AMD64 GNU 隔离虚拟机或可丢弃目录中工作，不以生产数据开始。
2. 记录 `sarmg-upgrade --version`、binary SHA、`support --json` 和 `catalog --json`。
3. 由对应 current 产品生成最小真实 fixture；不要自己手写一个“看起来像”的数据库。
4. 选择全新、同一文件系统的 output，执行 backup，并立即执行匹配的 verify。
5. 复制备份到另一隔离位置，做单字节篡改、extra file、错误 mode、错误 key 等拒绝实验。
6. 对全新目标执行 restore，再用产品 current offline doctor 验证，并在安全环境运行启动 smoke。
7. 只在产品明确支持 recover 时，使用故障注入留下 recovery，分别演练 commit 与 rollback。
8. 检查原件、来件、备份和 recovery 的每个身份是否仍可解释，最后才清理实验目录。

学习目标不是背命令，而是能回答：第一次 mutation 在哪一步、此时哪些事实已经 `fsync`、掉电后谁能证明
原代和来件、哪个命令被 support 授权、哪个动作必须停下来交给人工决策。

## 14. 阅读每章的方法

每章建议完成四件事：先读“边界”，再沿代码锚点找实现，然后画出失败时磁盘状态，最后写一个负例。
如果只能解释成功路径，尚未掌握备份/恢复代码。推荐在笔记中固定以下模板：

| 问题 | 必须记录的事实 |
|---|---|
| 输入是谁 | product、exact version、路径、文件 dev/ino/mode/nlink、key ID |
| 读取什么 | source、snapshot、manifest、tree inventory、SQLite sidecar |
| 第一次写哪里 | 私有 pending/stage，而不是正式 output/target |
| 第一次改变目标 | journal 已持久化之后的 original preserve |
| 成功证据 | published backup full verify 或 installed target current verify |
| 中断证据 | recovery path、journal phase、incoming/original hash 与位置 |
| 明确不证明 | service 已停、业务 smoke、未来版本兼容、历史迁移可用 |

## 15. 与其他文档的关系

- [工作流程与流程树](../project-workflow.md)用于逐阶段跟踪命令到代码和磁盘事实；
- [完整功能与取舍清单](../feature-inventory-and-tradeoffs.md)用于评审功能删除、复杂度和项目边界；
- [运维文档](../operations.md)用于真实变更窗口、命令、保管、恢复和安全事件；
- 仓库根 [README](../../README.md)用于快速确认平台、能力与目录入口。

本文和各章节不会把未来准入设计写成当前功能。凡涉及历史 edge 的段落都必须同时明确：当前
`upgrade_edges=[]`、没有 `upgrade-*` CLI、没有 source parser/转换 SQL/graph executor。Sentinel 与 Dufs
的 current adapter 已实现，但它们不构成历史升级 edge。
