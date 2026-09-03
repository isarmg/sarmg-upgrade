# Sarmg Upgrade 完整功能与取舍清单

本文描述 `sarmg-upgrade 0.2.0` 当前二进制实际提供的能力。项目仍在开发阶段，尚未形成可承诺的历史产品格式，因此当前支持矩阵中的 `upgrade_edges` 全部为空；仓库已删除 Host、Sunshine、Sentinel 和 Dufs 的试验性历史升级 SQL、适配器、source-backup 和 upgrade-recovery 命令。保留的是可复用的当前状态备份、严格校验、恢复、恢复日志和发布基础设施。

正式工具唯一支持 Linux AMD64 GNU `x86_64-unknown-linux-gnu`。它是停机离线 CLI，不是 Server，也没有
React/Vite 或其他前端；仓库当前无需运行时配置或服务部署，故不创建空 `config/`、`deploy/`、`clients/`。

## 1. 分类与复杂度

| 值 | 含义 |
|---|---|
| 核心 | 删除后工具不再能完成其当前承诺的备份/校验/恢复目标 |
| 保障 | 用户不一定直接调用，但负责身份、完整性、路径、锁、持久化或密钥安全 |
| 可选 | 特定产品或部署才需要，可在明确缩小范围后删除 |
| 建议保留 | 不是最小闭包，但显著降低事故或使用成本 |
| 开发运维 | 构建、测试、发行、诊断和文档能力 |

复杂度“低/中/高”表示连同 CLI、库 API、manifest、测试、发布和文档完成删除或变更的综合成本。下表每一
行都是一个可独立引用的开发决策边界：第二列定义当前实现与明确范围，第三列给出代码锚点，最后一列同时
给出最低正/负验证和不能外推的边界。某行写“当前为空”或“未实现”仍是有意产品边界，不是遗漏或计划中
功能；删除或改变这类限制同样需要评审。

## 2. 开发者决策台账

| ID | 功能/特性与当前实现/范围 | 实现/代码锚点 | 分类 | 复杂度 | 删除后的确定后果 | 验证与边界 |
|---|---|---|---|---|---|---|
| UPG-001 | `support` 机器可读支持矩阵 | `src/support.rs`、CLI、release | 核心 | 中 | 调用方无法区分“实现、未实现、计划中”；容易误调用不存在能力 | JSON 稳定性、每产品唯一、edge 全空 |
| UPG-002 | `catalog` 持久资源目录 | `src/catalog.rs`、Product/ResourceKind | 建议保留 | 低 | 运维和未来适配器缺少统一资源边界 | 六项目枚举、JSON/text 输出 |
| UPG-003 | `inspect-manifest` 严格只读解析 Foundation SQLite manifest | `src/main.rs`、`src/manifest.rs` | 建议保留 | 低 | 排障必须进入具体 verify/restore 才能发现 JSON 合同问题 | unknown field、坏相对路径、重复/乱序资源；不读资源、不验 hash/key/schema 字节，也不解析 Media composite manifest；当前 CLI 此入口没有独立文件大小上限 |
| UPG-004 | Media Backup 当前组合备份 | `src/current.rs`、SQLite online backup、tree copy | 核心 | 高 | Media 数据库与媒体树无法作为同一代保存 | DB/tree 交叉核对、并发源变化 |
| UPG-005 | Media v3 当前备份全树 inventory | tree root mode、非根目录 path/mode、文件 path/mode/size/SHA、tree digest | 保障 | 高 | 缺失、多余、mode 漂移或被篡改媒体无法在恢复前发现 | root/non-root mode、文件/目录/special/link/预算；不承诺 xattr/ACL/sparse/hardlink |
| UPG-006 | Media 当前备份严格 Schema identity | product metadata、schema fingerprint | 保障 | 高 | 错版本或手改数据库可能进入备份 | version/revision/SHA/DDL 负例 |
| UPG-007 | Media 当前备份原子发布 | 复用 `sqlite::PendingDirectory`、dirfd、fsync、`renameat2(RENAME_NOREPLACE)` | 保障 | 高 | 中断时可能暴露半成品或竞争覆盖既有备份 | 每个复制/manifest/fsync 故障点；竞争者创建 output 时发布失败且绝不覆盖，guard 清理由本次拥有的 pending |
| UPG-008 | `verify-media-backup` 全量只读复核 | current manifest、SQLite、tree hash | 核心 | 中 | 备份无法在灾难前证明可用 | tamper、extra/missing、wrong product |
| UPG-009 | `restore-media` DB+tree 组合安装 | restore stage、same-filesystem rename、journal | 核心 | 高 | 只能手工复制，容易形成 DB/tree 混合代 | 空目标、replace、跨设备、故障注入 |
| UPG-010 | Media restore commit/rollback 恢复 | recovery journal、`recover-media-restore` | 保障 | 高 | 崩溃后无法安全判断完成还是回退 | 各 mutation 点 commit/rollback |
| UPG-011 | Host Monitoring 当前 SQLite 备份 | `src/sqlite.rs`、official identity allowlist | 核心 | 高 | Host 当前数据库没有受支持备份路径 | online write、schema、integrity/FK |
| UPG-012 | Sunshine Manager 当前 SQLite 备份 | keyed SQLite path、ciphertext authentication | 核心 | 高 | Sunshine 当前数据库没有安全备份路径 | 正确/错误 key、全部密文认证 |
| UPG-013 | SQLite online backup API | rusqlite backup、maintenance shared lock | 保障 | 高 | 活跃库直接文件复制可能遗漏 WAL 或产生不一致快照 | WAL 写入并发、快照 identity |
| UPG-014 | SQLite backup manifest | `sarmg-contracts::BackupManifest` 线类型 + 本仓库产品策略包装、database size/SHA/schema | 保障 | 中 | 备份字节与产品/Schema/key 要求失去绑定，或各产品重新产生不一致 JSON | shared fixtures、manifest/path/hash/count 负例 |
| UPG-015 | Sunshine external key requirement 仅保存摘要 | key ID、key SHA、algorithm/version | 保障 | 中 | 把 raw key 放进备份会使单份泄露获得数据和解密能力 | manifest 不含 raw key、错 key 拒绝 |
| UPG-016 | 私有 key 文件读取 | `credentials_key_from_file`、no-follow、mode/link/race checks | 保障 | 高 | key 可从不安全或竞态路径读取 | symlink/hardlink/mode/变更/超限 |
| UPG-017 | Sunshine ciphertext 实际认证 | AES-256-GCM、hosts/operations 全行扫描 | 保障 | 高 | 只比较 key ID 会把错误 key 的备份标为可恢复 | host/operation、tamper、错 JSON |
| UPG-018 | `verify-sqlite` 不修改的全量验证 | SecureDirectory、manifest、DB verifier | 核心 | 中 | 不能在恢复前验证 SQLite 备份 | extra/missing/tamper/wrong product |
| UPG-019 | `restore-sqlite` exclusive maintenance | maintenance lock、current identity、stage | 核心 | 高 | 运行产品可与恢复并发，或需手工停机复制 | held lock、wrong product/version、replace |
| UPG-020 | SQLite sidecar generation 处理 | database/WAL/SHM/journal inventory | 保障 | 高 | 旧 sidecar 可污染新库或原库回滚不完整 | 每种 sidecar、extra name、identity |
| UPG-021 | SQLite durable restore journal | original/incoming hash、phase、fsync | 保障 | 高 | 断电后无法证明哪一代在目标路径 | journal tamper、每个 rename/fsync 点 |
| UPG-022 | `recover-sqlite` 显式 commit/rollback | recovery verifier、maintenance lock | 保障 | 高 | 自动猜测可能删除唯一完整代；无恢复则人工风险高 | incoming/original/abandoned 组合 |
| UPG-023 | 严格当前数据库对象集合；不为任何表名设置历史兼容特例 | `verify_current_database`、`sarmg-schema-identity` | 保障 | 中 | 若按特定“旧表名”分支处理，会让 schema identity 之外再出现一套隐含规则 | 官方 current fingerprint 精确匹配；任意额外/缺失/变更对象都会自然改变摘要并拒绝；不单独识别 `_sqlx_migrations` |
| UPG-024 | Schema 规范 fingerprint | `sarmg-schema-identity` 的 query、row model、UTF-8 length framing、SHA-256 | 保障 | 高 | 不同 DDL 可能冒充同 revision，或 rusqlite/SQLx 对同库算出不同摘要 | Foundation golden vector、顺序、internal objects、trigger/index |
| UPG-025 | 备份目录安全解析 | openat2/dirfd、no symlink、单链接普通文件 | 保障 | 高 | 恢复可被路径替换或读取特殊文件 | symlink/hardlink/FIFO/device/race |
| UPG-026 | 资源大小、条目、manifest 上限 | current/sqlite/manifest constants | 保障 | 中 | 恶意或损坏备份可耗尽内存/磁盘/时间 | 边界值与 overflow |
| UPG-027 | no-replace 输出语义 | pending UUID、RenameFlags::NOREPLACE | 保障 | 中 | 备份命令可能覆盖唯一已有证据 | existing dir/file/symlink |
| UPG-028 | `RecoveryAction` 显式选择 | CLI enum、journal verifier | 保障 | 中 | 工具若自动选择可能提交未验证目标或删除原代 | 非法值、commit 前验证 |
| UPG-029 | 通用 future `UpgradeEdge` 数据模型 | support types，当前为空 | 可选 | 低 | 删除只影响未来扩展；当前命令和支持能力不变 | 序列化空数组 |
| UPG-030 | 当前 Schema fixture 仅用于测试 | `tests/fixtures/current/*.sql` | 开发运维 | 中 | SQLite backup 测试无法构造精确官方库 | fixture fingerprint 等于 code allowlist |
| UPG-031 | Rust fmt/clippy/test 门禁 | Cargo、CI | 开发运维 | 中 | 路径/恢复状态机回归可进入发行 | all-targets、warnings denied |
| UPG-032 | source-bound release、SBOM、support snapshot | `scripts/`、workflow | 开发运维 | 高 | 制品支持范围、来源和依赖不可证明 | clean tag/SHA、archive、tamper |
| UPG-033 | 中文学习、流程、功能和运维文档 | README、`docs/` | 开发运维 | 低 | 操作者可能把开发期“无升级边”误解成自动升级 | 链接、CLI help、支持矩阵抽查 |
| UPG-034 | Sentinel/Dufs 当前组合备份完整闭包 | `backup-current`/`verify-current`/`restore-current`/`recover-current` | 核心 | 高 | 只处理 SQLite 会丢失配置、树和密钥绑定 | support JSON、exact resource set、配置同代替换与 recovery 集成测试 |
| UPG-035 | 所有历史升级边当前均未实现；未来稳定 edge 只归本仓库 | `upgrade_edges=[]`，无历史 SQL/adapter/CLI；未来准入必须在 `sarmg-upgrade` 以独立 adapter/fixture/CLI/release 原子加入 | 核心 | 高 | 当前用户必须重新部署开发数据；删除此边界或暗示另有迁移仓会诱发手工改库、current restore 冒充升级或职责分裂 | 当前源码/CLI/support 无历史 edge；未来变更同时具备精确 source/target、不可变 source backup、transform、故障矩阵、recovery、support 与 release，且产品 runtime 无兼容代码 |
| UPG-036 | Foundation 当前线协议绑定 | `sarmg-contracts`、`sarmg-schema-identity` 均精确 `=0.4.0` + Git rev `0e1be10273fd6abf72e0d0eeb24cbb1120572486`；`SchemaIdentity`/resource/external requirement 直接复用 | 保障 | 高 | 本工具会悄悄形成第二套字段、数值范围或枚举，跨项目备份无法可靠互认 | Foundation fixtures + 本仓库更严格负例 |
| UPG-037 | Driver-independent schema identity | `ProductMetadataRow/Column`、`SchemaRow`、canonical fingerprint | 保障 | 高 | DDL、列形状和摘要 framing 再次散落，算法修复无法一次覆盖所有消费者 | 空/多 metadata row、列漂移、fingerprint mismatch |
| UPG-038 | rusqlite 安全适配层 | `verify_schema_identity_database`、read-only open、integrity/FK、canonical schema query | 核心 | 高 | Foundation 会被迫依赖 rusqlite，或工具只验证自报 metadata 而不验证真实数据库 | 精确当前库、错 revision/hash、任意 schema drift；不含 migration-ledger 特判 |
| UPG-039 | 不可变 Foundation 依赖且无 fallback | Cargo exact `=0.4.0` + Git rev `0e1be10273fd6abf72e0d0eeb24cbb1120572486`；无 workspace sibling、path dependency、可变 branch、本地复制、feature fallback 或旧协议 alias | 开发运维 | 中 | 同一 sarmg-upgrade 源码可能随 Foundation 来源变化，或旧解析路径绕过当前合同 | `cargo metadata`、lockfile、clean checkout locked build；来源和 rev 必须逐字匹配 |
| UPG-040 | 正式发行唯一 target `x86_64-unknown-linux-gnu` | `src/support.rs::FORMAL_RELEASE_TARGET`、release scripts | 保障 | 中 | 非 AMD64/非 GNU 平台会被误认为受支持并进入事故矩阵 | `support --json.formal_release_target` 精确值；发布归档命名；不声明 ARM/musl/其他 OS |
| UPG-041 | 工具是离线 CLI，无 Server、daemon、HTTP API 或前端 | `src/main.rs`、仓库结构 | 核心 | 中 | 引入常驻服务会新增认证、网络、并发和密钥暴露面 | 只存在 CLI subcommand；无 listener、React/Vite、`clients/`；Dufs 前端例外与本工具无关 |
| UPG-042 | 当前无运行时配置和服务部署目录 | CLI 参数、仓库根 | 开发运维 | 低 | 新建空 `config/`/`deploy/` 会暗示不存在的配置或服务合同 | 所有产品/路径/key/动作逐次显式传入；无环境变量 fallback；未来确有配置再建立当前目录 |
| UPG-043 | `support` 同时输出 tool version、正式 target、排序 capability 与逐产品支持 | `src/support.rs` | 核心 | 中 | 自动化无法绑定二进制身份与精确能力 | 六产品各一次；capability 排序；current 四类列表；所有 edge 为空 |
| UPG-044 | Catalog 仅组合 Foundation 资源类别，不作实现支持声明 | `src/catalog.rs::Product::contract` | 建议保留 | 中 | 调用方可能把“产品有资源”误当“工具有命令” | Media DB+tree；Host/Sunshine SQLite；Sentinel 四资源；Dufs 三资源；Foundation 空 |
| UPG-045 | CLI 只注册当前 11 个命令族，不注册任何 `upgrade-*` | `src/main.rs::Command` | 核心 | 中 | 增加空壳命令会把计划误报为可用；删除发现命令会失去边界 | help 与 support 交叉核对；未知命令失败；无 alias、from/to 参数或自动图搜索 |
| UPG-046 | 所有选择由显式 product/version/path 参数驱动 | `src/main.rs`、`src/catalog.rs::FromStr` | 保障 | 中 | 内容推断会把相似库交给错误适配器 | product 只接受六个 canonical slug；SQLite restore/recover 与 Media recover 要求 expect-version；Media recover 还重给 source/DB/tree/recovery；无最近版本猜测 |
| UPG-047 | Foundation SQLite manifest 与 Media composite manifest 是两种当前合同 | `src/manifest.rs::BackupManifest`、`src/current.rs::CurrentBackupManifest` | 保障 | 高 | 混用 parser 会误把资源缺失或不同 journal 语义当有效 | `inspect-manifest` 只解析前者；`verify-media-backup` 只解析后者；均严格 unknown fields |
| UPG-048 | Foundation manifest 包装叠加产品、Schema、资源和 key 策略 | `src/manifest.rs::validate` | 保障 | 高 | 只校验 JSON 形状会接受错产品、自洽假 identity 或危险路径 | runtime 产品资源非空；SQLite identity 必需；version 一致；key requirement 精确 |
| UPG-049 | manifest 资源名/路径唯一、按名称严格递增、路径仅 normal relative component | `src/manifest.rs` | 保障 | 中 | 重复/乱序/穿越路径可覆盖或产生解析差异 | absolute、empty、`.`、`..`、duplicate name/path、unsorted 全拒绝 |
| UPG-050 | Foundation safe JSON integer、identifier 与 hash 边界向下游保留 | `sarmg-contracts`、`src/manifest.rs` | 保障 | 中 | JavaScript/其他消费者可能丢精度，宽松 hash/name 产生歧义 | MAX_SAFE_JSON_INTEGER、positive counts、lowercase SHA、unknown fields golden fixtures |
| UPG-051 | Media composite manifest 唯一 current version 3 与 current adapter ID | `src/current.rs::CURRENT_MANIFEST_VERSION`、`validate_manifest` | 保障 | 中 | 双读 v2/旧清单会重新引入旧 wire 合同，手写组合清单可能混入当前恢复 | version 3 正例；v2/unknown 拒绝且无 fallback；`media-backup-current-0.2.0-r1`、tool 0.2.0、product/version/identity 全精确 |
| UPG-052 | Media current Schema identity 固定 version 0.2.0/revision 1/SHA | `src/current.rs::product_contract` | 保障 | 中 | 错版本或 DDL 漂移进入 DB+tree 备份 | SHA `2563e6afc3fff272d02b7a5615272cc773862243bfd15aec51655abf1d9c6b1c`；metadata 与实际 schema 同验 |
| UPG-053 | Host current identity 固定 0.7.0/revision 1/SHA | `src/sqlite.rs::official_sqlite_identity` | 保障 | 中 | generic SQLite 误接 Host 旧库或手改库 | SHA `12dd1e61426b6b99df3d429b8c36ee3a5b22d1da776d98fc960b45b4f58c8e05`；fixture 与 code allowlist |
| UPG-054 | Sunshine current identity 固定 0.8.0/revision 2/SHA | `src/sqlite.rs::official_sqlite_identity` | 保障 | 中 | 密文校验前先接受错 schema，造成误读或漏扫 | SHA `c9dedb33dd7a5ad613e762eb135a7aa5184ce1df52166459bee7b3485b4b3be3`；fixture、integrity、FK |
| UPG-055 | schema fingerprint 对纳入范围的对象统一处理，无表名特判 | `verify_schema_identity_database`、`sarmg-schema-identity` | 保障 | 高 | 特判“旧表”会形成 Foundation identity 之外的第二套兼容规则 | type/name/tbl_name/sql canonical query/framing；任意 extra/missing/DDL drift 自然 mismatch |
| UPG-056 | Media 所有 database/tree/output 路径必须绝对且 output/tree 分离 | `src/current.rs::validate_options` | 保障 | 中 | cwd 变化或树内输出可造成递归复制、自包含和误删 | relative、output under tree、tree under output 拒绝；Media 不接受 key/config 参数 |
| UPG-057 | Media backup 先对 DB/tree 取得 exclusive lock，再验证源与产品业务不变量 | `src/current.rs::backup_current`、`ProductLocks` | 保障 | 高 | 复制前就可能接受混合产品或正在变化的 generation | 两个 sibling lock 均为 non-blocking exclusive；exact identity；复制完成后 destination inventory 与随后 source inventory 相等，不宣称有两次 source 快照；当前无 external key |
| UPG-058 | Media SQLite 使用 online backup，不复制 live main 文件 | `src/current.rs::copy_sqlite_snapshot` | 保障 | 高 | WAL 已提交数据可能遗漏，或复制到逻辑不一致页集 | concurrent WAL fixture、snapshot identity、integrity/FK、复制后再验证 |
| UPG-059 | Media tree 只接受目录和普通文件，复制后全量 v3 inventory | `copy_strict_tree`、`inventory_tree` | 保障 | 高 | symlink/special file 可逃逸，遗漏 root mode 会允许归档根权限静默漂移 | symlink/FIFO/socket/device/hardlink 拒绝；root mode、非根 path/mode、文件 path/mode/size/SHA、source/destination inventory 相等 |
| UPG-060 | Media tree budgets：2,000,000 entries、深度 128、manifest 128 MiB | `src/current.rs` constants | 保障 | 中 | 恶意或损坏树/manifest 可耗尽内存、fd 或时间 | limit/limit+1、深目录、overflow、超大 manifest；文件内容仍受目标磁盘容量约束 |
| UPG-061 | Media source tree identity hash 记录源物理路径 identity | `path_identity_sha256`、`CurrentBackupManifest` | 保障 | 中 | 操作者可能把另一棵相似树的语义混成同一来源 | manifest 字段格式与 current validation；不是内容 hash，也不替代 tree inventory |
| UPG-062 | Media 输出先写私有 UUID pending，manifest 最后 create-new，最终 dirfd no-replace publish + parent sync | `sqlite::PendingDirectory`、`write_json_create_new`、`backup_current` | 保障 | 高 | 中断暴露半备份，或竞争覆盖既有证据 | output 必须不存在；race creator 正例；每个 copy/write/fsync/rename 故障；失败 guard 清理 owned pending；成功后再次 verify |
| UPG-063 | Media verify 对 backup 根 exact 三项并全量复核 DB、tree、空 configuration/external requirements 与业务状态 | `src/current.rs::verify_current_backup` | 核心 | 高 | 只验 manifest 或忽略顶层 extra 会把未纳入 generation 的资源当可恢复 | 根目录只允许 database.sqlite3/tree/manifest.json；extra/missing/type/tamper/root 与 entry mode/hash/schema/tree relation；configuration/external requirements 必须空；只读输入，无 repair |
| UPG-064 | Media restore 要求目标 DB/tree 同代：均不存在或均存在且显式 replace | `src/current.rs::restore_current` | 核心 | 高 | 可产生 database/tree 混合代 | mixed existence 拒绝；existing 必须 exact current；Media configuration 参数必须空；空目标演练 |
| UPG-065 | Media restore 只为 DB/tree 建相邻 incoming 与 original | `src/current.rs::restore_current`、`RestoreJournal` | 保障 | 高 | 跨文件系统安装不原子；原件无法成组回滚 | sibling 路径、same filesystem rename、DB/tree 同代切换、无 copy fallback；journal 的通用 configuration vector 在当前 Media 合同中必须为空 |
| UPG-066 | Media current journal v2 绑定 source/target/generation 并持久化六个 phase | `src/current.rs::{RestoreJournal,RestorePhase,CURRENT_RESTORE_JOURNAL_VERSION}` | 保障 | 高 | 中断后无法证明哪一代在哪个名字；旧 journal 双读会重新引入历史合同 | tool/product/version/adapter/Schema/time；source backup canonical path+dev/inode、manifest version/time/bytes/hash、source tree identity；target+parent identity；同 nonce stage/original；incoming/optional original DB/tree 完整 inventory；configuration/external requirements 均 `[]`；六 phase；unknown/缺字段、非 v2、非法 phase 拒绝；最大 1 MiB、目录 fsync |
| UPG-067 | `recover-media-restore` 要求显式 current version、source backup、DB/tree target、recovery 与 commit/rollback | `src/main.rs::Command::RecoverMediaRestore`、`src/current.rs::recover_current` | 保障 | 高 | 从不可信 journal 自动猜 source/target/action 或无锁续接可能修改错误 generation | 六项 `--expect-version/--input/--database/--data-dir/--recovery/--action` 必填并与 journal 精确一致；canonical/disjoint 路径与 recovery simple UUID 名；DB/tree 两把 non-blocking exclusive lock；pending journal 丢弃；commit 全量 verify；rollback 方向不可逆且恢复 original 或移除无原代 incoming；重复同 action 幂等 |
| UPG-068 | SQLite-only 只允许 Host 或 Sunshine | `src/sqlite.rs::require_sqlite_only_product` | 核心 | 中 | Media/Sentinel/Dufs 组合资源会被 generic SQLite 假装完整备份 | 三组合产品负例；support/capability 不出现；不能手工改 product slug |
| UPG-069 | SQLite backup 目录必须恰好有 database.sqlite3 与 manifest.json | `src/sqlite.rs::verify_sqlite_backup_internal` | 保障 | 中 | extra 文件可能隐藏 sidecar/恶意状态或造成歧义 | exact entry vector；extra/missing/type/symlink 拒绝；manifest 最大 1 MiB |
| UPG-070 | SQLite backup 在 shared maintenance lock 下使用 online backup | `MaintenanceLock::shared`、`copy_database_online` | 保障 | 高 | 与产品 writer 并发或直接 cp 产生不一致 generation | lock contention、WAL concurrent write、source/snapshot identity 一致 |
| UPG-071 | SQLite pending 输出 create-new/no-clobber，发布后重跑 verify | `PendingDirectory`、`create_sqlite_backup_internal` | 保障 | 高 | 覆盖备份或发布未验证目录 | existing file/dir/symlink、copy/manifest/fsync/rename fault、post-publish verify |
| UPG-072 | SQLite manifest database resource 精确 name/kind/path/files=1 | `verify_sqlite_backup_internal` | 保障 | 中 | 任意资源布局会被 restore 误解 | resource count=1；bytes/SHA 与实际 file；schema identity 与 DB/manifest 一致 |
| UPG-073 | SecureDirectory 通过 dirfd/openat2 拒绝链接与特殊对象 | `src/sqlite.rs::SecureDirectory` | 保障 | 高 | 用户可在验证和打开间替换路径或注入 FIFO/device | symlink/hardlink/special/race、entry names、no-follow fd；只读 verify 不修改 |
| UPG-074 | SQLite restore 取得 exclusive maintenance lock | `src/sqlite/restore.rs` | 保障 | 高 | 产品 writer 可与切换并发，造成 sidecar/页集混合 | held shared/exclusive locks、service stopped 前提；工具不负责 systemd |
| UPG-075 | SQLite restore 对目标现存 main/`-wal`/`-shm`/`-journal` 成组保存 | `DatabaseLocation`、restore journal originals | 保障 | 高 | 旧 sidecar 污染 incoming 或 rollback 不完整 | 每种 sidecar 存在组合、hash/size、single-link regular、rename+sync |
| UPG-076 | SQLite journal 记录 incoming hash、destination 与 originals 后才安装 | `src/sqlite/restore.rs::RestoreJournal` | 保障 | 高 | 断电后无法证明目标属于哪一代 | journal version/unknown fields/tamper；prepared/preserved/installed 故障点 |
| UPG-077 | SQLite rollback 把已安装 incoming 保存在 abandoned-new 后恢复 originals | `resume_rollback` | 保障 | 高 | rollback 可能销毁唯一可调查的新代 | destination 必须证明为 original 或 incoming；NOREPLACE；恢复后 current verify |
| UPG-078 | SQLite commit 证明 incoming/target 与每个 original 的唯一位置 | `resume_commit` | 保障 | 高 | commit 可能接受手工改动或丢 original | hash/bytes/path exact；歧义组合拒绝；完成后 verify 与 cleanup |
| UPG-079 | `recover-sqlite` CLI 当前只允许 Host Monitoring | `src/main.rs::Command::RecoverSqlite` | 核心 | 中 | 若对 Sunshine开放会绕过 external key ciphertext 验证 | Host 0.7.0 正例；Sunshine/其他 product 负例；support recover 列表一致 |
| UPG-080 | Sunshine restore 可执行，但中断 recovery 不作为支持能力 | `restore_sqlite_backup_with_credentials`、`src/support.rs` | 保障 | 高 | 冒充可 recover 会诱导调用 Host 路径或手工拼接密文 DB | support recover 为空；残留目录保全并停止；不得调用 recover-sqlite |
| UPG-081 | key ID 1～64 ASCII alnum/`-_`，key file base64 解码为精确 32 bytes | `validate_key_id`、`credentials_key_from_file` | 保障 | 中 | 模糊 key identity 或错误长度进入 AES-256-GCM | empty/65/非法字符/base64/31/33 bytes 负例；文件最大 4096 bytes |
| UPG-082 | key 文件必须私有、单硬链接、普通文件，读取前中后复核 identity | `credentials_key_from_file` | 保障 | 高 | 攻击者替换/共享 Secret 或用特殊文件阻塞 | group/other bits、symlink/hardlink、dev/ino/size/mtime race；raw key 不输出 |
| UPG-083 | Sunshine manifest 只保存 key ID、key SHA、AES-256-GCM、envelope v1 | `sunshine_external_requirement` | 保障 | 中 | 把 raw key 放入同一备份会失去信任域分离 | manifest/JSON/debug/log 无 raw bytes；错误 ID/hash/algorithm/version 拒绝 |
| UPG-084 | Sunshine 全量扫描并认证所有非 NULL host/operation 密文 | `verify_sunshine_encrypted_values` | 保障 | 高 | 只比 key SHA 无法证明 key 能解密全部当前状态；按 operation 完成状态漏扫会留下未认证行 | hosts.secret 与 operations.request_ciphertext 的所有非 NULL 行；correct/wrong key、tamper、malformed envelope/JSON；12-byte nonce + 产品/对象/action/字段长度分帧 AAD |
| UPG-085 | verify 命令严格只读且不修复 manifest、DB 或 tree | verify functions、SecureDirectory | 核心 | 中 | 自动 repair 会销毁事故证据或掩盖来源问题 | 输入 mode/hash/mtime 可比对；失败无持久 mutation；错误信息不泄露 key |
| UPG-086 | backup/restore 与 recover 是不同授权动作 | CLI enums、support matrix | 保障 | 中 | 一个通用“自动继续”命令会在证据不足时替操作者决策 | restore requires replace flag；recover requires explicit commit/rollback；unsupported action 拒绝 |
| UPG-087 | 所有具体历史 `upgrade_edges` 当前为空 | `src/support.rs`、CLI、无 adapters/SQL | 核心 | 高 | 若文档或 release 暗示 edge，用户会把开发数据交给不存在的路径 | 每产品 empty；help 无 upgrade；source search 仅允许类型/文档/未来准入描述 |
| UPG-088 | `UpgradeEdge` 只是未来支持矩阵的数据结构，不是执行引擎 | `src/support.rs::UpgradeEdge` | 可选 | 低 | 删除只影响未来元数据扩展，当前 backup/restore 不变 | 只序列化空数组；无 graph search、adapter registry、source/target executor |
| UPG-089 | 可复用基础设施当前只包括 current backup、verify、stage、journal、recover 原语；未来 edge 复用但不改变其 current 语义 | `src/current.rs`、`src/sqlite.rs`、`src/sqlite/restore.rs` | 建议保留 | 高 | 删除会迫使未来 edge 重建安全原语；称为“已支持迁移”会误导；把 edge 赶到别的仓库又会拆散统一迁移/备份/恢复责任 | 只能按 support 暴露当前 capability；任何未来稳定 edge 必须在本仓库以独立 adapter/fixture/CLI/release 加入，不能直接扩宽 current parser |
| UPG-090 | 不自动停止/启动产品或重启 watchdog | CLI 与 docs，无 service integration | 保障 | 中 | 自动化服务控制错误可在恢复中重启 writer | 运维先停服务并禁止拉起；maintenance lock 是最后防线，不是 service manager |
| UPG-091 | 不跨文件系统 copy fallback，不覆盖已有备份 | rename/NOREPLACE paths | 保障 | 高 | copy fallback 可暴露半代；覆盖会销毁唯一证据 | EXDEV、existing destination、parent sync fault；操作者更换同盘路径后重试新输出 |
| UPG-092 | Rust 1.98、edition 2024 与锁文件是当前构建基线 | `rust-toolchain.toml`、`Cargo.toml`、`Cargo.lock` | 开发运维 | 中 | 工具链漂移令安全 lint、依赖和制品不可复现 | exact toolchain、`--locked`、all-targets/features；不宣称其他 Rust 版本 |
| UPG-093 | CI 分离普通质量与 source-bound release | `.github/workflows/ci.yml`、`release.yml` | 开发运维 | 高 | release 可能绕过测试或在 publish job 执行源码 | fmt/check/clippy/test/supply-chain；publish job 不 checkout；artifact provenance |
| UPG-094 | stage/finalize release 生成 support/catalog、SBOM、provenance、checksum/signature，并把 Secret 私钥锁定到源码公钥 | `scripts/stage-release.sh`、`finalize-release.sh`、`write-sbom.py`、`release/sarmg-upgrade-release-signing-public.pem` | 开发运维 | 高 | 使用者无法证明二进制来源、依赖或能力；任意私钥都能产生“自带公钥”的伪信任链 | clean annotated tag、exact SHA、no-clobber asset、源码公钥 DER 指纹、私钥派生公钥逐字节匹配、解包复验、错误 key/tamper 负例 |
| UPG-095 | 文档只保留 README、初学者、流程树、功能取舍、运维五类 | `README.md`、`docs/` | 开发运维 | 低 | 平行历史设计文档会与 current support 漂移 | 中文内容、本地链接、命令与 help/support 抽查；English 仅用于准确术语 |

### 2.1 Foundation 与本工具的责任边界

| 关注点 | Foundation 负责 | sarmg-upgrade 负责 | 为什么不能合并 |
|---|---|---|---|
| Backup manifest 线格式 | 字段、serde unknown-field 策略、safe JSON integer、SHA/identifier、资源与外部要求基础类型 | `product` 必须属于目录；SQLite 必须有 identity；版本必须一致；路径安全；资源名/路径唯一且有序；Sunshine key 要求 | 通用线格式不能硬编码某产品版本，本工具也不能自行放宽线格式 |
| Schema identity | 四字段类型、identifier/hash 校验、exact comparison | 为 Host、Sunshine、Media 提供 code-owned 官方 current identity | Foundation 不决定哪个产品版本受支持 |
| `product_metadata` | 五列模型、canonical DDL、单行转换与列形状校验 | 通过 rusqlite 安全读取真实表；执行 integrity/FK 检查 | 共享 crate 保持 driver-independent，避免同时链接 rusqlite 与 SQLx SQLite |
| Schema fingerprint | 排除项、排序 query、四字段 UTF-8/u64 big-endian framing、SHA-256 golden vector | 执行 query、把 rusqlite row 转成共享 `SchemaRow`、比对数据库 metadata 与官方 allowlist | 算法必须跨驱动唯一；文件与连接生命周期属于产品工具 |
| Current-only 策略 | 类型只有一个当前 wire version，不提供 legacy alias | 拒绝错版本、未知 Schema、自洽但非官方 Schema；任何对象变化由同一 canonical fingerprint 处理；支持矩阵 edge 为空 | 是否接受某数据库属于产品/工具策略，不属于基础库；不得为历史表名增加特殊分支 |
| 文件系统与恢复 | 不负责 | openat2/dirfd、hardlink/symlink 防护、lock、snapshot、fsync、journal、commit/rollback | 这些行为依赖产品路径、权限和故障模型 |

Foundation 删除或修改共享字段/算法时，本仓库必须在同一变更中更新精确依赖、fixtures 和负例；不能临时
复制旧实现。反过来，本仓库增加产品版本、backup adapter 或历史 edge，不得把产品表、密钥或恢复策略塞入
Foundation。

## 3. 当前支持矩阵

| 产品 | 当前 backup/verify/restore | recover | 历史升级边 | 外部要求 |
|---|---|---|---|---|
| Media Backup `0.2.0` | DB + data tree 组合状态 | commit/rollback | 无 | 无 |
| Host Monitoring `0.7.0` | SQLite | commit/rollback | 无 | 无 |
| Sunshine Manager `0.8.0` | SQLite + 密文认证 | restore 中断恢复当前不对外声明 | 无 | credentials key ID + 32-byte key |
| Sentinel Monitor `0.2.0` | DB + recordings + 三个配置 + 密文认证 | `recover-current` commit/rollback | 无 | credentials key ID + 32-byte key |
| Dufs RAM `0.50.1` | DB + shared root + `dufs.yaml` | `recover-current` commit/rollback | 无 | 无 |
| Sarmg Foundation | 无运行时状态 | 不适用 | 无 | 不适用 |

机器调用必须以 `sarmg-upgrade support --json` 为准。表格不能让未实现命令变成支持能力。

## 4. 删除历史升级代码的取舍

开发期产品格式还会变化。保留具体 `0.x -> 0.y` 适配器会产生四种错误成本：把试验数据结构误当长期合同；迫使当前产品继续保留旧语义；让安全验证同时覆盖多代密文和 Schema；让文档与发布矩阵宣称未经现实迁移验证的能力。因此当前删除了历史 SQL、旧格式 parser、source manifest、产品升级 journal 和相关 CLI。

未来首个历史升级适配器必须在独立提交中重新加入，并至少具备：精确 source/target 身份、拒绝自动版本猜测、先发布不可变 source backup、从零构建 target、外部 key 实际认证、停机锁、容量预算、durable journal、所有持久边界故障注入、commit/rollback、support/catalog/release 元数据和中文演练文档。

## 5. 通用恢复状态机

```text
严格验证当前备份
  -> 取得 maintenance exclusive lock
  -> 在目标同目录创建私有 recovery
  -> 复制并验证 incoming
  -> 保存 original database + sidecars
  -> fsync journal 和目录
  -> 原子安装 incoming
  -> 再验证当前 Schema/密文
  -> 清理 original/recovery

任一步骤中断
  -> 保留 journal 与证据
  -> 操作者显式选择 commit 或 rollback
  -> 工具先核对 hash、identity、目标和锁，再继续
```

“replace existing”不是覆盖授权的同义词：它只允许在完整原代已经进入 recovery 且 journal 持久化后切换。跨文件系统 copy fallback、自动选择 commit、自动删除 recovery 均不提供。

## 6. 明确不提供

- 自动发现版本和搜索升级图；
- 开发期历史 Schema/API/密文兼容；
- 在线零停机 migration；
- 自动停止或启动 systemd、watchdog 或其他进程监督器；
- 任意 SQLite 文件备份；
- 把 credentials key 放入备份；
- 跨文件系统的非原子安装；
- Sentinel 或 Dufs 的 generic SQLite-only 恢复；
- 自动清理无法判定的 recovery 证据；
- 宽松接受未知 manifest 字段或资源。

## 7. 功能完成定义

代码存在一个复制函数不代表功能完成。新功能只有在支持矩阵、严格 manifest、路径与文件身份、锁顺序、容量预算、snapshot、target 验证、external key、journal、故障注入、commit/rollback、CLI、release provenance、SBOM 和中文演练文档全部完成后，才可列为支持。
