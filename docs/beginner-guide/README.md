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

Media Backup `0.2.0` 支持数据库与 data tree 组合备份；Host Monitoring `0.7.0` 和 Sunshine Manager
`0.7.0` 支持严格 SQLite 备份。Sentinel、Dufs 当前组合备份尚未实现，所有历史升级 edge 也未实现。
自动化必须读取 `support --json`，不能根据 catalog 或本文推断命令。

## 5. 为什么先复制再解析

当前 SQLite 备份使用 SQLite online backup 取得逻辑一致快照，随后验证当前 Schema identity。未来历史
adapter 若加入，必须先在 exclusive maintenance lock 下保存 source 证据，再从零构建 target；这只是
准入规则，不是当前二进制能力。

## 6. 为什么不能只复制 app.db

WAL 模式下已提交数据可能仍在 `-wal`，只复制主文件会生成逻辑旧快照。Media Backup 还包括媒体树；
Sunshine 依赖 external credential key。Sentinel/Dufs 虽在 catalog 中声明组合资源，但当前未实现对应
命令，不能用通用 `cp` 或 SQLite 命令替代。

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
