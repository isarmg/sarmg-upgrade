# Sarmg Upgrade 工作流程与流程树

## 1. 当前流程树

```text
sarmg-upgrade 0.2.0
├─ 平台：仅 x86_64-unknown-linux-gnu 离线 CLI；无 Server、无前端
├─ 能力发现
│  ├─ support --json
│  ├─ catalog --json
│  └─ inspect-manifest
├─ Media Backup 0.2.0 当前组合状态
│  ├─ backup-media
│  ├─ verify-media-backup
│  ├─ restore-media
│  └─ recover-media-restore
├─ 当前 SQLite-only 状态
│  ├─ Host Monitoring 0.7.0：backup / verify / restore / recover
│  └─ Sunshine Manager 0.7.0：keyed backup / verify / restore
├─ Foundation 共享合同：两个 crate 均为 =0.3.0
│  └─ immutable Git rev：1fe326081cfd896f05ff502e80f99504797c14c6
│  ├─ sarmg-contracts：manifest / resource / external requirement / SchemaIdentity
│  └─ sarmg-schema-identity：metadata row/column / canonical query / fingerprint
├─ 暂未实现
│  ├─ Sentinel 当前组合备份/恢复
│  ├─ Dufs 当前组合备份/恢复
│  └─ 所有历史升级 edge
└─ 交付
   ├─ tests + clippy + supply-chain checks
   └─ source-bound archive + support snapshot + SBOM + provenance
```

## 2. 能力发现先于命令执行

```text
验证 binary/release checksum
 -> sarmg-upgrade support --json
 -> 确认 product + operation + exact current version
 -> sarmg-upgrade catalog --json
 -> 确认资源是 SQLite-only 还是 composite
 -> 不在 support 中：立即停止，不用相近命令替代
```

`catalog` 表示产品拥有哪些持久资源，不表示工具已经实现该产品的完整备份。唯一机器事实是当前 binary
输出的 `support`；当前每个 `upgrade_edges` 数组都必须为空。

## 2.1 共享合同进入数据库验证的调用树

```text
sarmg-upgrade 产品命令
 -> 本仓库路径、文件身份、锁和 SQLite read-only connection
 -> rusqlite 读取 pragma_table_info / product_metadata / sqlite_schema
 -> sarmg-schema-identity 校验列和单行 metadata
 -> sarmg-schema-identity 对 SchemaRow 执行唯一 canonical fingerprint
 -> 本仓库比对 code-owned 产品 + current version + revision + SHA allowlist
 -> sarmg-contracts 生成/解析当前 backup manifest
 -> 本仓库叠加路径、资源唯一/排序、产品 key 和恢复策略
```

边界是刻意的：Foundation 不链接 rusqlite、不打开产品路径、不声明 Host/Media/Sunshine 的支持版本；本仓库
不复制 schema framing、metadata 五列模型、manifest leaf type 或旧 wire parser。开发、CI 与正式发行均只允许
同时具有 `=0.3.0` 和不可变 Git rev `1fe326081cfd896f05ff502e80f99504797c14c6` 的 Foundation 依赖，
并把来源与 lockfile 纳入审核；不得用 workspace sibling、Cargo path dependency、可变 branch 或本地副本联调。

## 3. 当前备份流程

```text
校验显式 product/路径/key
 -> 取得产品约定的 maintenance lock（Media 组合备份为 exclusive；SQLite-only 为 shared）
 -> 验证 code-owned 当前 Schema identity
 -> Sunshine：实际认证全部密文
 -> 创建私有 pending output（目标必须不存在）
 -> SQLite online snapshot；组合产品再复制 data tree
 -> 核对文件 type/mode/size/SHA/tree inventory
 -> 再验证 copied state
 -> 最后写 manifest 并 fsync
 -> directory-FD renameat2(RENAME_NOREPLACE) 发布 output；竞争目标绝不覆盖
 -> 重新执行对应 verify
```

Media 的 database 与 data tree 是一个组合代，不能拆成两个命令。Host/Sunshine 才能使用 SQLite-only 路径。

## 4. 当前恢复流程

```text
严格解析 manifest
 -> 验证每个资源、Schema、Hash 和 external requirement
 -> 取得 exclusive maintenance lock
 -> 核对目标版本和 replace policy
 -> 在目标同一文件系统创建 incoming stage
 -> 再验证 incoming
 -> 创建相邻私有 recovery + durable journal
 -> 保存原 database+tree 或 database+sidecars（按 adapter 且仅在允许 replace 时）
 -> 原子安装 incoming
 -> 验证已安装当前代
 -> fsync 并清理 recovery
```

恢复不自动停止或启动产品。操作者必须先阻止 systemd、watchdog、其他进程监督器或手工进程
重启目标服务。工具的锁是最后一道并发保护，不是服务管理器。

## 5. 中断恢复流程

```text
命令报告 recovery path
 -> 保持产品停止
 -> 不编辑/移动/删除 recovery，也不移动/替换原 source backup
 -> 修复空间、只读挂载等环境问题
 -> 使用相同 binary/product/version/path
 -> action=commit：证明 incoming 后完成安装
 -> action=rollback：证明 preserved original 后恢复
 -> verify + 产品 doctor + smoke
```

Host 的 SQLite restore 可用 `recover-sqlite`。Media 使用 `recover-media-restore`。Sunshine 的 keyed
SQLite restore 当前不对外声明 recover；若出现无法自动清理的 recovery，停止并保全证据，不要用 Host
命令绕过密文认证。

Media recovery 不从 journal 自动选择上下文：CLI 必须重新提供 exact `--expect-version 0.2.0`、source
`--input`、目标 `--database/--data-dir`、`--recovery` 和 `--action commit|rollback`。工具比对 v2 journal 后，对目标 DB/tree 两个
sibling lock 取得 non-blocking exclusive 锁，再验证 source manifest、路径 identity、incoming/original
inventory 与 phase。source backup 绑定规范绝对路径和目录 dev/inode，不能在中断后移动、替换或拿内容相同
的副本顶替。CLI 不暴露可伪造的 product/runtime 参数：命令内部把 product 固定为 `media-backup`，当前
runtime directory 固定为 absent，并把这两个 current 上下文交给 `CurrentRecoveryOptions`。journal 绑定 tool
version 而不是 binary SHA，制品摘要仍由运维变更单证明。

## 6. 为什么当前没有升级流程

开发期数据格式不是长期合同。试验性历史 SQL/adapter 会迫使产品和工具维护尚未承诺的旧语义，因此已
删除。当前流程树不存在 `upgrade-sqlite`、`upgrade-sentinel`、`upgrade-dufs`、source-backup 或
upgrade-recovery 分支。旧开发数据应重新部署；当前没有已支持 edge 时，`sarmg-upgrade` 也不得用 current
restore 或临时脚本冒充迁移。未来稳定版本的精确 edge 只能在本仓库以独立审核的 adapter、fixture、CLI
与 release 原子加入，绝不并回产品运行时，也不为模糊旧输入增加 alias 或 fallback。

未来首个 edge 的准入流程为：稳定 source/target 身份 -> 独立 fixture 和恶意负例 -> immutable source
backup -> 从零构建 target -> external key -> 停机锁 -> durable journal -> 全故障点 commit/rollback ->
support/release/docs。任何一步未完成，都不得出现在支持矩阵。

## 7. 正式发行流程

```text
clean checkout + annotated exact tag
 -> fmt/check/clippy/test
 -> 运行 Foundation contract fixture 与 schema golden-vector 的下游集成测试
 -> workflow supply-chain validation
 -> build source-bound binary
 -> capture support/catalog
 -> generate SBOM/environment/provenance
 -> immutable build artifact
 -> publish job 不 checkout source
 -> sign SHA256SUMS
 -> 解包复验 binary/support/checksum
 -> 发布固定 archive；拒绝覆盖既有 asset
```

发布后必须抽查 `support --json`：若出现未经审核的历史 edge，发行失败。

## 8. 命令到代码的精确分派

| CLI | 选择条件 | 核心实现 | 成功证明 | 明确不证明 |
|---|---|---|---|---|
| `support [--json]` | 无产品输入 | `support_matrix()` | 此 binary 编译进的 current capability 与正式 target | catalog 中的未来资源已经实现 |
| `catalog [--json]` | 无产品输入 | `Product::contract()` | 六个产品的持久资源边界 | 某资源已有 backup/restore adapter |
| `inspect-manifest PATH` | Foundation SQLite manifest | `BackupManifest::read` | JSON、共享字段、产品策略、相对路径、排序可解析 | Media manifest、资源字节、hash、Schema、key；此入口当前不单独限制读取字节数 |
| `backup-media` | Media 0.2.0 | `backup_current` | DB+tree 同一 current generation 已不可变发布并复验 | 任意其他 Media 版本或历史转换 |
| `verify-media-backup` | Media current composite backup | `verify_current_backup` | DB identity、tree inventory、manifest 全部相符 | 源生产路径此刻仍与备份相同 |
| `restore-media` | 全新目标或显式 replace | `restore_current` | current DB+tree 已成组安装并验证 | 服务已停止、业务 smoke 已通过 |
| `recover-media-restore` | 显式 `--expect-version/--input/--database/--data-dir/--recovery/--action` 六项 | `recover_current` | 显式 current 上下文与严格 journal/磁盘证据一致后，选择的 commit/rollback 完成 | 省略 source/target/recovery/action、随意更换 binary/路径或编辑 journal |
| `backup-sqlite` | 仅 Host/Sunshine current | `create_sqlite_backup*` | online snapshot、identity、hash；Sunshine 还证明密文可认证 | Media/Sentinel/Dufs 已完整备份 |
| `verify-sqlite` | 仅 Host/Sunshine current backup | `verify_sqlite_backup*` | exact 两文件目录、manifest、DB、key 合同 | 目标路径可安全 replace |
| `restore-sqlite` | Host/Sunshine exact expect-version | `restore_sqlite_backup*` | 当前 DB 已按 durable journal 安装并验证 | Sunshine 中断 recovery 已受支持 |
| `recover-sqlite` | 仅 Host 0.7.0 | `recover_sqlite_restore` | original/incoming 位置和 hash 可证明后完成动作 | 可用于 Sunshine 或其他产品 |

CLI 不读取运行时配置文件，也没有环境变量 fallback。仓库因此没有空的 `config/`、`deploy/` 或 `clients/`：
这不是遗漏，而是离线 CLI 当前边界。若未来增加常驻 Server 或前端，必须作为新的产品面重新设计，不能把
别的项目 React/Vite、认证或部署目录机械复制进来。

## 9. Schema identity 验证树

```text
显式 product
 -> 只读打开真实 SQLite
 -> integrity_check 与 foreign_key_check 通过？
 -> 校验 product_metadata 精确五列且恰好一行
 -> 转换为 Foundation ProductMetadataRow
 -> 查询 Foundation canonical sqlite_schema rows
 -> 对 type/name/tbl_name/sql 做 u64 big-endian length framing
 -> 计算 SHA-256
 -> metadata 自报 identity 与实际摘要一致？
 -> 与 binary code-owned official identity 完全一致？
 -> 允许进入下一阶段
```

三套当前 allowlist 为：

| 产品 | version | revision | schema SHA-256 |
|---|---:|---:|---|
| Media Backup | `0.2.0` | 1 | `2563e6afc3fff272d02b7a5615272cc773862243bfd15aec51655abf1d9c6b1c` |
| Host Monitoring | `0.7.0` | 1 | `12dd1e61426b6b99df3d429b8c36ee3a5b22d1da776d98fc960b45b4f58c8e05` |
| Sunshine Manager | `0.7.0` | 1 | `a717bcd5a591e7f7cc6da5826af88ad0deab2fdc339ce4649ad84f21ea879dbc` |

任何额外表/index/trigger、缺失对象或 DDL 变化都通过同一 fingerprint 自然拒绝。代码没有、也不应新增
针对 `_sqlx_migrations` 或其他“旧表名”的特殊分支；否则 current identity 之外又会出现隐式兼容规则。

## 10. Media current backup 的持久边界

```text
validate absolute/disjoint paths
 -> database/tree 两个 non-blocking exclusive ProductLocks
 -> verify source DB identity + Media DB/tree relation
 -> create private .<output>.pending-UUID
 -> SQLite online backup -> verify snapshot identity
 -> create private tree/ -> strict recursive copy
 -> inventory copied tree and live source; must be equal
 -> construct unique v3 current manifest（含 tree root mode）
 -> validate manifest -> create-new manifest.json
 -> fsync pending directory
 -> rename pending to previously absent output
 -> fsync output parent
 -> full verify of published output
```

Media manifest 只接受 version 3，不兼容读取 version 2。backup 根必须恰好有 `database.sqlite3`、`tree/`、
`manifest.json` 三项。树只接受目录和普通文件；symlink、FIFO、socket、device 等立即失败。inventory 绑定
tree 根 mode、非根目录相对 path/mode、普通文件 path/mode/size/SHA-256，并把它们纳入聚合摘要；条目最多
2,000,000、深度最多 128，manifest 最多 128 MiB。`source_tree_identity_sha256` 记录源目录物理 identity，
不能代替内容 inventory。命令成功仅表示输出代通过验证，不表示源之后没有变化。

## 11. Media restore 与 recovery phase

Media journal 的唯一 current version 是 2，最大 1 MiB；不读取 v1，也不补默认字段。字段逐项绑定：

- `tool_version`、`product`、`application_version`、`adapter_id`、完整 `schema_identity` 与
  `created_at_epoch_seconds`；
- `source_backup` 规范绝对路径及 `source_backup_identity_sha256`，以及 source manifest 的
  `source_manifest_version`、`source_manifest_created_at_epoch_seconds`、`source_manifest_bytes`、
  `source_manifest_sha256` 和 `source_tree_identity_sha256`；
- `database`、`tree` 两个规范绝对目标路径及 `database_path_identity_sha256`、
  `tree_path_identity_sha256`；
- 由同一个 recovery nonce 精确推导的 `database_stage`、`tree_stage`、`database_original`、
  `tree_original` sibling 路径；
- incoming database/tree 的完整内容 inventory，以及目标原本存在时 optional original database/tree 的完整
  内容 inventory；
- 必须精确为空的 `configuration`、`external_requirements` 和下列六个 `phase`。

source、recovery、database、tree 都必须由 CLI 显式给出且是 canonical absolute path；source 与 targets 必须
disjoint。recovery 必须是数据库同级的 `.<db>.recovery-<32位小写十六进制 simple UUID>`，stage/original
名称必须由同一个 nonce 推导，不能接受“形状相似”的相邻目录。recover 先结构校验 journal 并比对显式
source/targets/version，再取得 database sibling 与 tree sibling 两把 non-blocking exclusive `ProductLock`，
随后才验证 source inode/path identity、manifest exact hash/identity、stage/target/original 的 DB
SHA-256/mode/Schema 及 tree inventory/per-file SHA-256/BLAKE3/SQLite 对应关系。锁内发现 pending journal
update 时，把它视为尚未提交的更新并丢弃，不能拿它覆盖已提交 journal 状态。

recovery 目录顶层也执行 exact 校验：只允许单硬链接、最大 1 MiB 的 `restore-journal.json`，以及同样受限的
optional `restore-journal.pending`；额外条目、链接、特殊文件或缺少 committed journal 一律拒绝。

| Phase/事实 | 磁盘状态 | 此时中断后的原则 |
|---|---|---|
| pre-journal | incoming 尚在 sibling stage，目标未改 | 删除本次 stage 后可重新从 verify 开始；不得把 stage 当成功恢复 |
| `prepared` | recovery journal 已持久，原目标仍在或目标原本为空 | commit 可安装 incoming；rollback 在无原代时只清理由本次建立的代 |
| `originals-preserved` | 原 DB/tree 已改名到 original，目标名暂空 | 禁止启动产品；commit 安装 incoming，rollback 成组恢复 original |
| `installed` | incoming 已占目标名，original 仍保留 | 必须再次验证；不能仅因目标存在就删除 recovery |
| `verified` | 目标 current generation 已验证 | commit cleanup 可删除 original/recovery；仍需目录同步成功 |
| `rollback-started` | 操作者已选择回退，journal 已持久化该方向 | 之后不能改选 commit；继续证明并恢复 original，或移除原先为空目标的 incoming |
| `rollback-verified` | 回退后的原代或“目标应不存在”状态已验证 | 只允许完成同步与 recovery cleanup，不重新安装 incoming |

未知 phase、phase 与现场证据不一致、旧/缺字段 journal 都失败关闭。相同 action 可在中断后幂等推进；一旦
`rollback-started` 已持久化，后续调用不得改选 commit。每次删除 original、stage 或 recovery 前都重新验证
对应证据，不能把早先一次检查当作永久授权。

目标 DB 和 tree 必须同时不存在，或同时存在且显式 `--replace-existing`；混合存在直接拒绝。现存 generation
还必须本来就是精确 current，工具不会用 restore 顺便“修复”未知库。Media current adapter 明确拒绝
configuration 参数，v3 manifest 的 `configuration` 必须为空，backup 根也只允许三项；不得从通用 journal
结构中预留的空 configuration vector 推断 Media 已包含配置资源。

## 12. SQLite-only backup、restore 与 sidecar 流程

```text
explicit Host/Sunshine product
 -> shared maintenance lock
 -> verify official source identity
 -> Sunshine: authenticate every encrypted value with supplied key
 -> private pending directory
 -> SQLite online backup to database.sqlite3
 -> verify snapshot identity (+ Sunshine ciphertext)
 -> hash database -> strict Foundation manifest
 -> fsync -> no-clobber publish -> full verify

restore
 -> verify exact two-file backup directory
 -> exclusive maintenance lock
 -> inspect target main + -wal/-shm/-journal as one generation
 -> copy incoming next to target and verify
 -> durable restore journal containing hashes and original entries
 -> preserve originals -> install incoming -> verify
 -> cleanup only after proof
```

SQLite backup 目录必须恰好包含 `database.sqlite3` 与 `manifest.json`，manifest 最大 1 MiB。extra 文件不是
“忽略即可”的注释，而是失败；它可能是未纳入 generation 的 sidecar 或攻击者内容。restore 会把当时存在的
main/`-wal`/`-shm`/`-journal` 全部作为 original entries 保存，commit/rollback 均以 hash、size、位置证明，
而不是按文件名猜测。

## 13. Sunshine external key 流程

```text
private key file
 -> lstat: regular + nlink=1 + no group/other bits + <=4096 bytes
 -> O_NOFOLLOW open
 -> dev/ino/mode/size/mtime before/after stable
 -> UTF-8 Base64 trim/decode -> exactly 32 bytes
 -> key ID 1..64 [A-Za-z0-9_-]
 -> compare manifest key requirement
 -> authenticate all current encrypted host/operation rows
```

manifest 只保存非秘密 `kid`、key SHA-256、`aes-256-gcm`、envelope version 1。key SHA 只绑定外部要求，
不能替代真正解密认证。Sunshine 支持 backup/verify/restore，但 `support` 不声明 recover；restore 中断必须保全
证据并停止，不能调用 Host 的 `recover-sqlite`。

## 14. 失败结果的操作含义

| 失败位置 | 可安全做什么 | 禁止什么 |
|---|---|---|
| support/catalog 不含能力 | 立即停止并重新部署或保全输入；未来只能等待本仓库精确 edge 完整准入 | 用相似产品/generic SQLite 命令或临时脚本替代 |
| backup pending 尚未发布 | 修复环境后选择全新 output 重跑 | 把 pending 改名冒充完整备份 |
| verify 失败 | 保全输入，复制到隔离处调查 | 修改 manifest/hash/metadata 让它通过 |
| restore 在 journal 前失败 | 确认目标未变后清理本次 stage | 未验证就启动产品 |
| restore 报告 recovery | 保持停服，用相同 binary 和显式 action | 编辑/移动 recovery，自动猜 commit/rollback |
| Sunshine recovery 残留 | 保全目录、日志、key ID 与 binary SHA，人工升级处置 | 使用 Host recovery 或手工 cp main 文件 |
| EXDEV/目标已存在 | 选择同文件系统全新路径或新 output | 添加 copy fallback、覆盖已有 backup |

## 15. 未来 edge 的准入边界

`UpgradeEdge { from, to }` 当前只是支持矩阵的未来数据结构；所有数组为空。仓库保留的未来可复用基础设施仅指
可复用的 immutable backup、strict verify、same-filesystem stage、durable journal 和 explicit recovery 原语，
不包含 graph search、历史 parser、转换 SQL、adapter registry 或 upgrade CLI。首个 edge 只有在 source/target
identity、不可变 source backup、从零 target 构建、组合资源/key、故障注入、support/release/docs 全部落地后
才是实现；在此之前不得以“引擎已具备”为由对外承诺历史迁移。
