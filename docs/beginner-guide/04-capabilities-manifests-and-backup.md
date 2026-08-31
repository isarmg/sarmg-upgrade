# 04. 能力目录、Manifest 与备份流程

## 4.1 `support --json`

输出当前 binary 真正实现的 command、产品、版本/edge 和所需能力。自动化在执行前保存此快照，并拒绝
不存在的能力；不要从文档标题猜命令。

`src/support.rs::support_matrix` 输出四层事实：工具版本、正式 target、排序后的 capability 字符串，以及每个
产品的 current backup/verify/restore/recover 版本列表、历史 edge 与 external requirement。调用方应保存
完整 JSON，而不是只匹配某个字符串。一个安全的自动化判定必须同时确认 product、operation、version 和
formal target，且不得在缺失时回退到 catalog 或硬编码默认。

当前矩阵的关键不对称是：Media 和 Host 有 recover，Sunshine 没有；Sentinel、Dufs、Foundation 的四个
current 列表均为空；六个产品的 `upgrade_edges` 均为空。不能把一个产品的能力复制到另一个产品。

## 4.2 `catalog --json`

catalog 描述产品的当前状态版本、Schema、数据库、树、config、companion 和 external key 要求。它可
包含尚无 adapter 的产品，因此不是执行授权。

| 产品 | catalog 中的完整资源 | 当前 support 结论 |
|---|---|---|
| Media Backup | SQLite + data tree | 有专用 composite adapter |
| Host Monitoring | SQLite | 有 SQLite-only adapter |
| Sunshine Manager | SQLite + external credentials key 要求 | 有 keyed SQLite-only adapter |
| Sentinel Monitor | SQLite + configuration + companion contract + recordings | 无 adapter |
| Dufs RAM | SQLite + data tree + configuration | 无 adapter |
| Sarmg Foundation | 空，无 runtime state | 无需 adapter |

catalog 的作用是阻止“只备份数据库就算完整”的误解。例如 Dufs 有 SQLite，但同时有 protected YAML 与 shared
root；所以 generic SQLite 命令必须拒绝 Dufs，而不是因为资源列表中含 SQLite 就允许。

## 4.3 Manifest 作用

manifest 把 backup 身份、工具版本、产品/版本、资源、文件 mode/size/Hash、tree aggregate、预算和非秘密
external requirement 固化。它不包含 raw key，也不单凭自报内容获得信任。

SQLite-only manifest 的线格式来自 Foundation `sarmg-contracts =0.3.0`，其不可变 Git rev 为
`1fe326081cfd896f05ff502e80f99504797c14c6`。本工具不是把共享 JSON
复制一遍，而是直接包装共享 `BackupManifest`，并直接复用 `BackupResource`、
`BackupExternalRequirement`、`StateResourceKind` 与 `SchemaIdentity`。共享层保证字段名、unknown-field
拒绝、identifier/SHA 和 JavaScript safe-integer 边界一致；本工具再验证以下产品事实：

- `product` 必须能解析为当前 catalog 中的精确产品；
- SQLite resource 必须带 schema identity，且 identity 的 application/version 与清单相符；
- resource path 必须是仅含 normal component 的相对路径；
- resource name 与 path 不重复，且资源按 name 严格排序；
- Sunshine 当前清单必须且只能声明约定的 credentials-key；其他 SQLite-only 产品不能偷带外部要求；
- 最终仍要比对 code-owned official current version/revision/schema SHA，而不是信任清单自报。

当前有两种彼此独立的 manifest：

| 合同 | 使用范围 | 固定布局 | 正确验证入口 |
|---|---|---|---|
| Foundation `BackupManifest` 的本仓包装 | Host/Sunshine SQLite-only | `database.sqlite3` + `manifest.json` | `verify-sqlite` |
| `CurrentBackupManifest` version 3 | Media composite | exact `database.sqlite3` + `tree/` + `manifest.json` | `verify-media-backup` |

`inspect-manifest` 只调用第一种 parser。当前该命令直接读取给定文件，没有独立的 1 MiB 文件大小保护；
1 MiB 上限在 SQLite backup 目录的完整 verify 路径执行。因此不要对任意不可信超大文件使用
`inspect-manifest`，也不要把 parse 成功写成“备份已验证”。Media manifest 最大 128 MiB，必须通过 Media
专用入口解析和复核。Media 只读 v3，不为 v2 保留 alias/fallback；v3 把 tree root mode 纳入 inventory。

## 4.4 Manifest 不是信任根

把 manifest 理解为“待证明的声明集合”更准确：

1. parser 先证明 JSON 形状、字段、数值和相对路径满足线协议；
2. adapter 再证明 product、version、resource set、external requirement 与 code allowlist 一致；
3. verifier 从磁盘重算 bytes/SHA/tree inventory；
4. SQLite verifier 从实际库重算 integrity、FK、metadata 和 Schema fingerprint；
5. Sunshine verifier用独立 key 实际认证持久密文；
6. 只有全部相等，backup 才是当前 binary 可恢复的输入。

攻击者完全可以同时修改数据库和 manifest 中的自报 SHA。code-owned schema SHA、产品专用业务不变量和
external key 认证就是为了阻止“自洽但伪造”的输入。

## 4.5 严格解析

拒绝 unknown fields、重复 key、非法数值、非规范路径、绝对/父穿越、超长集合和不受支持算法/version。
解析后仍要按 code allowlist 比对产品合同。

Schema fingerprint 也不由 rusqlite adapter 自行发明。Foundation 给出 `product_metadata` 五列模型、唯一
schema-row query，以及每个 `type/name/tbl_name/sql` UTF-8 字段前置 unsigned 64-bit big-endian 字节长度的
framing；adapter 只把查询结果转换成共享 `SchemaRow`。这样 SQLx 产品与 rusqlite 工具面对同一 schema
不会因为遍历顺序、字符长度或拼接歧义得出不同摘要。

Foundation manifest 还固定安全 JSON integer、identifier、lowercase SHA-256、resource count/bytes 等通用
边界。本仓包装额外要求 resource name/path 唯一、name 严格递增、路径只含 normal relative component，
SQLite resource 必须有与 product/application version 一致的 `SchemaIdentity`。unknown field 不是“忽略后
继续”，而是格式不受支持。

这种严格性意味着给 manifest 加一段自定义备注也会失败。运维说明、存储层 checksum 和变更单应放在
backup 目录之外，避免改变 exact entry set。

## 4.6 SQLite-only 备份时序

```text
parse explicit product/path/key pair
 -> require Host or Sunshine exact SQLite-only product
 -> acquire shared maintenance lock
 -> verify live source exact current identity
 -> Sunshine: authenticate all required ciphertext
 -> create private pending output
 -> SQLite online backup to database.sqlite3
 -> verify snapshot identity and ciphertext again
 -> hash bytes and create Foundation manifest
 -> fsync files/directory
 -> no-clobber publish output
 -> run full verify on published output
```

最后一步很重要：backup 命令返回前会对发布后的目录再次执行对应 verify。即使如此，长期保管期间仍要周期
复验，因为存储介质、权限和外部 key 可用性会变化。

## 4.7 Media 组合备份时序

```text
validate args -> acquire canonical locks -> prove current identity/key
 -> create private pending output -> snapshot all resources
 -> compute inventory/Hash -> verify copy -> manifest last
 -> fsync -> rename to requested output -> fsync parent
```

Media 当前没有 external key/config，图中的 key/config 是通用函数位置，不是 Media 能力。实际流程还会：

- 验证数据库内每条 blob 引用的 account/blob storage path 都是安全相对路径；
- 确认树中对应文件为单硬链接普通文件；
- 比对数据库 `stored_size` 与文件长度，以及数据库 `content_blake3` 与文件内容；
- 用 SQLite online backup 生成 snapshot；
- strict copy tree，并比较复制后 tree inventory 与 source 的再次 inventory；
- manifest 最后 create-new，pending 同步后才发布，再运行完整 `verify_current_backup`。

数据库业务行与 tree 的交叉检查弥补了纯 SHA inventory 的盲点：一棵内部自洽但不对应数据库引用的树不是
有效 Media generation。

## 4.8 备份期间的源

online backup 只有产品合同明确允许 maintenance shared 时才可使用；组合资源通常需要停止应用/companion
并取得更多排他锁。工具不自动停止服务。

Host/Sunshine SQLite-only backup 使用 maintenance shared lock 和 SQLite online backup；Media 会对 database
与 tree 两个合同 lock 都取得 non-blocking exclusive lock，因为还要复制普通文件树，并在复制完成后分别
取得 destination 与随后 source 的 inventory 做相等比较。它不是两次 source 快照；排他锁和停服边界负责
排除未被该单次比较捕获的并发变化。两种路径都按停机 CLI 运维：停止产品、companion 与自动拉起，再让
工具锁提供最后一道并发拒绝。

## 4.9 Verification

`verify-*` 重新读取所有资源、mode/Hash/tree、SQLite integrity/FK/Schema/metadata，并在需要时用 external
key 认证全部密文。只校验 manifest checksum 不够。

verify 的只读边界是：不修复数据库、不补文件、不重写 manifest、不更换 key requirement，也不删除 extra
entry。失败后输入仍是调查证据。若运维希望重新生成备份，应使用原可信 source 和一个全新 output，不能
在失败 backup 内手工替换单个资源。

## 4.10 空间预算

运行前估算 source logical/physical bytes、pending copy、target stage、preserved original、WAL 和 recovery。
tree budgets 是本次授权并写入合同，不使用“无限”值绕过。

当前显式解析上限包括 SQLite manifest 1 MiB、credentials key file 4096 bytes、Media manifest 128 MiB、
Media tree 2,000,000 entries/深度 128。上限不是容量规划：一个合法的巨大普通文件仍需要对应磁盘空间、
读取时间和 Hash 时间。生产变更还要按源逻辑/物理大小估算 pending、restore incoming、preserved original
和备份传输副本。

## 4.11 失败清理

mutation 前的 private pending 可按工具证明安全后清理；已发布 output 不覆盖；任何涉及原目标 mutation 的
失败保留 recovery evidence。清理策略不能只看名字匹配。

backup 阶段尚未触碰生产目标，`PendingDirectory` guard 会清理由本次创建且身份已知的 pending；最终通过
directory-FD `renameat2(RENAME_NOREPLACE)` 发布，竞争者抢先创建 output 时也绝不覆盖。kill/掉电残留不能
仅凭名称自动删。restore 一旦创建 durable recovery 并开始 mutation，证据优先于整洁，必须交给对应 recover
或事件流程。不要写 cron 按 `.pending`、`.recovery` 名称通配删除。

## 4.12 代码阅读路线

1. `src/support.rs`：能力如何从实际 current version 生成。
2. `src/catalog.rs`：资源组合为何不等于 adapter。
3. `src/manifest.rs`：共享线类型与本仓产品策略的边界。
4. `src/sqlite.rs::create_sqlite_backup_internal`：Host/Sunshine 时序。
5. `src/current.rs::backup_current`：Media DB+tree 时序。
6. 两个完整 verifier：观察每个自报字段如何被实际资源重新证明。

## 4.13 本章检查

应能说明 `support`、`catalog`、`inspect-manifest` 和 `verify-*` 各自回答什么、不能回答什么；两种 manifest
为何不能混用；Media 为什么不能走 generic SQLite；以及为什么 manifest 最后写、目录同步、no-clobber
publish 和发布后复验缺一不可。
