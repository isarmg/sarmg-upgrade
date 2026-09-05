# Sarmg Upgrade

`sarmg-upgrade 0.3.0` 是 Sarmg 产品的离线 current-state 备份、验证与恢复工具。仓库记录未来历史 edge 的
完整准入条件，但当前 binary 不是升级引擎。业务产品只创建并接受自身当前版本，不携带旧 Schema reader、
自动 migration、兼容 alias、backup writer 或 restore code。

正式二进制唯一支持 Linux AMD64 GNU，即精确 target `x86_64-unknown-linux-gnu`；其他 CPU、OS 与 ABI
不属于构建或运维边界。它是停机 CLI，不是 Server，也没有前端/客户端，因此 React/Vite 约定不适用。
仓库当前不需要运行时配置与部署单元，故意不创建空的 `config/`、`deploy/` 或 `clients/`；产品数据库、
数据树、key、输入和输出路径都由每次 CLI 显式提供。

项目仍处于开发阶段，当前没有任何历史升级边；`support --json` 的 `upgrade_edges` 全部为空，二进制也不
提供 `upgrade-*` 命令。已实现 Media、Sentinel 和 Dufs 的当前组合状态，以及 Host、Sunshine 的当前
SQLite 状态备份/验证/恢复。备份不可变、带摘要且不覆盖；恢复先暂存验证，再通过持久 journal 切换。

平台化 P0 已在 `tests/fixtures/sources/<product>/<version>/` 冻结未迁移产品的脱敏 current-state source
fixture。每套 SQLite fixture 都包含管理员、有效 Session、Unicode/长度边界业务数据和审计记录，并由测试
复算精确 Schema fingerprint 与外键完整性。Dufs 的静态管理员和内存 Session 另以配置及行为 Golden 文件
表达。已退役的 Sunshine 0.7 夹具已删除；当前代码不包含历史 parser 或兼容路径。

跨项目线协议来自 Foundation 0.4.0：`sarmg-contracts` 与 `sarmg-schema-identity` 均使用精确版本。
`sarmg-contracts` 是当前 backup manifest、资源类别和
`SchemaIdentity` 的唯一 Rust 线类型，`sarmg-schema-identity` 是 `product_metadata` 形状、schema row
查询及 SHA-256 framing 的唯一算法实现。本仓库只实现 rusqlite 读取适配器和更严格的产品策略，例如精确
current identity、相对路径、资源唯一/排序、密钥要求、文件系统防护与恢复状态机；Foundation 不读取产品
数据库、不决定支持版本，也不提供迁移边。开发、CI 与发行都只接受上述 exact version + immutable rev；
不得改用 workspace sibling、Cargo path dependency、可变 branch、本地副本、宽松 parser 或旧版本 fallback。

## 当前能力边界

下表是本仓库当前代码的人工摘要；自动化和生产变更仍必须以正在执行的二进制
`sarmg-upgrade support --json` 为准。

| 产品 | 精确 current identity | 已实现命令族 | 中断 recover | 外部要求 |
|---|---|---|---|---|
| Media Backup | `0.2.0` / revision 1 / `2563e6afc3fff272d02b7a5615272cc773862243bfd15aec51655abf1d9c6b1c` | composite backup、verify、restore | `recover-media-restore` | 无 |
| Host Monitoring | `0.8.0` / revision 1 / `12dd1e61426b6b99df3d429b8c36ee3a5b22d1da776d98fc960b45b4f58c8e05` | SQLite backup、verify、restore | `recover-sqlite` | 无 |
| Sunshine Manager | `0.8.0` / revision 2 / `c9dedb33dd7a5ad613e762eb135a7aa5184ce1df52166459bee7b3485b4b3be3` | keyed SQLite backup、verify、restore | 不对外支持 | key ID 与独立 32-byte credentials key |
| Sentinel Monitor | `0.2.0` / revision 1 / `f547ddc817d830d23b5305bb1f88b29898d6531568edd6eb194c2b629eb560c0` | composite current | `recover-current` | 三个配置文件与当前 credentials key |
| Dufs RAM | `0.50.1` / revision 1 / `3659ff0c703515f555af95f0f1c08c35fa0555a8978f5f0e5a658fd93d225423` | composite current | `recover-current` | `dufs.yaml` |
| Sarmg Foundation | 无运行时状态 | 无 | 不适用 | 源码/制品由 Git 与 package 流程管理 |

所有产品的 `upgrade_edges` 均为空。仓库中存在 `UpgradeEdge` 线类型以及可复用的备份、验证、同文件系统
stage、durable journal 和显式 recovery 原语，不等于存在升级执行引擎：当前没有历史 source parser、迁移
SQL、adapter registry、graph search、`from/to` 选择器或 `upgrade-*` CLI。旧开发数据默认重新部署；当前
没有受支持 edge 时不得用 current restore 冒充迁移。未来产品版本稳定且确有长期迁移需求时，精确 edge
只能在本 `sarmg-upgrade` 仓库中以独立 adapter、fixture、CLI 与 release 原子加入，不能把兼容分支放回
业务产品或现有 current adapter，也不能加入版本猜测、宽松 parser 或 fallback。

## 命令地图

```text
只读能力发现
├─ support [--json]
├─ catalog [--json]
└─ inspect-manifest MANIFEST

Media current composite state（保留的专用命令是 generic current 命令的等价入口）
├─ backup-media --database ABS --data-dir ABS --output ABS
├─ verify-media-backup --input ABS
├─ restore-media --input ABS --database ABS --data-dir ABS [--replace-existing]
└─ recover-media-restore --expect-version 0.2.0 --input BACKUP_ABS
   --database DB_ABS --data-dir TREE_ABS --recovery RECOVERY_ABS
   --action commit|rollback

Media/Sentinel/Dufs current composite state
├─ backup-current --product PRODUCT --database ABS --data-dir ABS --output ABS
│  [--configuration NAME=ABS ...] [key options]
├─ verify-current --product PRODUCT --input ABS [key options]
├─ restore-current --product PRODUCT --input ABS --database ABS --data-dir ABS
│  [--configuration NAME=TARGET_ABS ...] [--replace-existing] [key options]
└─ recover-current --product PRODUCT --expect-version VERSION --input ABS
   --database DB_ABS --data-dir TREE_ABS --recovery RECOVERY_ABS
   --action commit|rollback [key options]

Host/Sunshine current SQLite-only state
├─ backup-sqlite --product PRODUCT --database ABS --output ABS [key options]
├─ verify-sqlite --product PRODUCT --input ABS [key options]
├─ restore-sqlite --product PRODUCT --expect-version VERSION --input ABS --database ABS
│  [--replace-existing] [key options]
└─ recover-sqlite --product host-monitoring --expect-version 0.8.0
   --recovery ABS --action commit|rollback
```

`inspect-manifest` 只严格解析 Foundation SQLite manifest；它不读取资源字节、不复算 SHA/schema、不验证
Sunshine key，也不解析 Media composite manifest。`catalog` 只回答“产品的完整持久状态由哪些资源组成”，
不回答“当前二进制是否实现 adapter”。只有 `support` 可以授权选择命令。

Composite manifest 的唯一 current wire version 是 3；不读取 version 2，也不保留双 parser。备份
根目录必须恰好包含 manifest 声明的数据库、数据树和配置资源。v3 的 tree inventory 把 tree 根 mode、
各非根目录 mode、文件 path/mode/size/SHA 和聚合摘要全部绑定；顶层 extra entry、tree 中的链接/特殊文件、
任一 mode 或内容漂移都会使 full verify 失败。

## 不可删减的安全不变量

- 所有产品、版本、路径和 key 都由显式参数选择；不按目录名、表名或“最近版本”猜测。
- 备份 output 必须原先不存在；先写私有 pending，完整验证并同步后才 no-clobber 发布。
- SQLite live source 使用 online backup 取得一致 snapshot，不直接复制可能处于 WAL 模式的 main file。
- manifest 自报 identity 不是信任根；工具同时重算真实 `sqlite_schema` fingerprint，并与 binary 内置 current
  allowlist 精确比较。
- restore 在第一次改变目标前，必须把 incoming、original identities 和 phase 写入 durable journal；
  `--replace-existing` 只授权进入保全原代的流程，不授权直接覆盖。
- stage/incoming/recovery 与目标位于同一文件系统；发生 `EXDEV` 时拒绝，不添加 copy fallback。
- 工具不负责停止、启动或屏蔽 systemd/watchdog；操作者必须先停服，maintenance lock 只是最后防线。
- verify 永远只读，不修复 metadata、manifest、Hash、tree 或密文；失败输入作为证据保全。
- Sunshine/Sentinel raw key 不进入 manifest、备份、stdout、Debug 或日志；key file 还必须满足普通文件、单硬链接、
  私有权限、稳定文件身份和精确 32-byte 解码边界。
- recovery 路径一旦报告就不得手工移动、编辑、拼接或删除；仅在对应产品明确支持时，用同一 binary 和显式
  `commit`/`rollback` 继续。

## 仓库结构

| 路径 | 责任 |
|---|---|
| `src/main.rs` | CLI 参数、命令分派和 product/key 组合拒绝 |
| `src/support.rs` | 当前二进制能力的唯一机器可读目录；所有历史 edge 为空 |
| `src/catalog.rs` | 六个产品的完整持久资源合同，不作实现承诺 |
| `src/manifest.rs` | Foundation SQLite manifest 的产品级严格包装 |
| `src/current.rs` | Media/Sentinel/Dufs current 组合备份、验证、恢复和 recovery |
| `src/sqlite.rs` | Host/Sunshine current SQLite snapshot、identity、manifest 与安全文件层 |
| `src/sqlite/restore.rs` | SQLite restore journal、original/sidecar 保全、commit/rollback |
| `tests/` 与模块内测试 | current identity、恶意输入、并发、故障和 CLI/release 回归 |
| `tests/fixtures/sources/` | 当前未迁移产品的脱敏 P0 source fixture；不作为旧版本兼容输入 |
| `scripts/` | supply-chain 检查与 source-bound 正式发行 |
| `docs/` | 仅保留初学者、流程、完整功能取舍和运维分类 |

本仓库没有 Server、HTTP API、前端、运行时配置或部署单元，所以不创建虚假的 `clients/`、`config/`、
`deploy/`。若未来职责发生变化，应先重新定义产品边界，而不是为了目录外观一致加入空壳。

## 快速验证

```bash
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 check --locked --all-targets --all-features
cargo +1.98.0 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.98.0 test --locked --all-targets --all-features
./scripts/check-workflow-supply-chain.py
```

先用机器可读命令确认当前二进制能力：

```bash
sarmg-upgrade support --json
sarmg-upgrade catalog --json
```

`support --json` 的 `formal_release_target` 必须精确为 `x86_64-unknown-linux-gnu`。`catalog` 描述完整资源合同，
`support` 描述当前二进制实际可执行的 adapter。

## 文档

- [文档总览](docs/README.md)
- [初学者学习指南](docs/beginner-guide/README.md)
- [项目工作流程与流程树](docs/project-workflow.md)
- [完整功能与取舍清单](docs/feature-inventory-and-tradeoffs.md)
- [备份、恢复、安全与发行运维](docs/operations.md)

代码采用 [Apache License 2.0](LICENSE-APACHE)。
