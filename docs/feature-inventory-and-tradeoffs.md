# Sarmg Upgrade 完整功能与取舍清单

本文描述 `sarmg-upgrade 0.2.0` 当前二进制实际提供的能力。项目仍在开发阶段，尚未形成可承诺的历史产品格式，因此当前支持矩阵中的 `upgrade_edges` 全部为空；仓库已删除 Host、Sunshine、Sentinel 和 Dufs 的试验性历史升级 SQL、适配器、source-backup 和 upgrade-recovery 命令。保留的是可复用的当前状态备份、严格校验、恢复、恢复日志和发布基础设施。

## 1. 分类与复杂度

| 值 | 含义 |
|---|---|
| 核心 | 删除后工具不再能完成其当前承诺的备份/校验/恢复目标 |
| 保障 | 用户不一定直接调用，但负责身份、完整性、路径、锁、持久化或密钥安全 |
| 可选 | 特定产品或部署才需要，可在明确缩小范围后删除 |
| 建议保留 | 不是最小闭包，但显著降低事故或使用成本 |
| 开发运维 | 构建、测试、发行、诊断和文档能力 |

复杂度“低/中/高”表示连同 CLI、库 API、manifest、测试、发布和文档完成删除或变更的综合成本。

## 2. 开发者决策台账

| ID | 功能/特性与当前实现 | 实现/主要依赖 | 分类 | 复杂度 | 删除后的确定后果 | 最低验证 |
|---|---|---|---|---|---|---|
| UPG-001 | `support` 机器可读支持矩阵 | `src/support.rs`、CLI、release | 核心 | 中 | 调用方无法区分“实现、未实现、计划中”；容易误调用不存在能力 | JSON 稳定性、每产品唯一、edge 全空 |
| UPG-002 | `catalog` 持久资源目录 | `src/catalog.rs`、Product/ResourceKind | 建议保留 | 低 | 运维和未来适配器缺少统一资源边界 | 六项目枚举、JSON/text 输出 |
| UPG-003 | `inspect-manifest` 严格只读解析 | manifest parser、CLI | 建议保留 | 低 | 排障必须进入具体 restore 才能发现格式问题 | unknown field、超限、坏路径/hash |
| UPG-004 | Media Backup 当前组合备份 | `src/current.rs`、SQLite online backup、tree copy | 核心 | 高 | Media 数据库与媒体树无法作为同一代保存 | DB/tree 交叉核对、并发源变化 |
| UPG-005 | Media 当前备份全树 inventory | file mode/size/SHA、tree digest | 保障 | 高 | 缺失、多余或被篡改媒体无法在恢复前发现 | 文件/目录/special/link/预算 |
| UPG-006 | Media 当前备份严格 Schema identity | product metadata、schema fingerprint | 保障 | 高 | 错版本或手改数据库可能进入备份 | version/revision/SHA/DDL 负例 |
| UPG-007 | Media 当前备份原子发布 | private pending directory、fsync、no-replace rename | 保障 | 高 | 中断时可能暴露半成品或覆盖既有备份 | 每个复制/manifest/fsync 故障点 |
| UPG-008 | `verify-media-backup` 全量只读复核 | current manifest、SQLite、tree hash | 核心 | 中 | 备份无法在灾难前证明可用 | tamper、extra/missing、wrong product |
| UPG-009 | `restore-media` DB+tree 组合安装 | restore stage、same-filesystem rename、journal | 核心 | 高 | 只能手工复制，容易形成 DB/tree 混合代 | 空目标、replace、跨设备、故障注入 |
| UPG-010 | Media restore commit/rollback 恢复 | recovery journal、`recover-media-restore` | 保障 | 高 | 崩溃后无法安全判断完成还是回退 | 各 mutation 点 commit/rollback |
| UPG-011 | Host Monitoring 当前 SQLite 备份 | `src/sqlite.rs`、official identity allowlist | 核心 | 高 | Host 当前数据库没有受支持备份路径 | online write、schema、integrity/FK |
| UPG-012 | Sunshine Manager 当前 SQLite 备份 | keyed SQLite path、ciphertext authentication | 核心 | 高 | Sunshine 当前数据库没有安全备份路径 | 正确/错误 key、全部密文认证 |
| UPG-013 | SQLite online backup API | rusqlite backup、maintenance shared lock | 保障 | 高 | 活跃库直接文件复制可能遗漏 WAL 或产生不一致快照 | WAL 写入并发、快照 identity |
| UPG-014 | SQLite backup manifest | `BackupManifest`、database size/SHA/schema | 保障 | 中 | 备份字节与产品/Schema/key 要求失去绑定 | manifest/path/hash/count 负例 |
| UPG-015 | Sunshine external key requirement 仅保存摘要 | key ID、key SHA、algorithm/version | 保障 | 中 | 把 raw key 放进备份会使单份泄露获得数据和解密能力 | manifest 不含 raw key、错 key 拒绝 |
| UPG-016 | 私有 key 文件读取 | `credentials_key_from_file`、no-follow、mode/link/race checks | 保障 | 高 | key 可从不安全或竞态路径读取 | symlink/hardlink/mode/变更/超限 |
| UPG-017 | Sunshine ciphertext 实际认证 | AES-256-GCM、hosts/operations 全行扫描 | 保障 | 高 | 只比较 key ID 会把错误 key 的备份标为可恢复 | host/operation、tamper、错 JSON |
| UPG-018 | `verify-sqlite` 不修改的全量验证 | SecureDirectory、manifest、DB verifier | 核心 | 中 | 不能在恢复前验证 SQLite 备份 | extra/missing/tamper/wrong product |
| UPG-019 | `restore-sqlite` exclusive maintenance | maintenance lock、current identity、stage | 核心 | 高 | 运行产品可与恢复并发，或需手工停机复制 | held lock、wrong product/version、replace |
| UPG-020 | SQLite sidecar generation 处理 | database/WAL/SHM/journal inventory | 保障 | 高 | 旧 sidecar 可污染新库或原库回滚不完整 | 每种 sidecar、extra name、identity |
| UPG-021 | SQLite durable restore journal | original/incoming hash、phase、fsync | 保障 | 高 | 断电后无法证明哪一代在目标路径 | journal tamper、每个 rename/fsync 点 |
| UPG-022 | `recover-sqlite` 显式 commit/rollback | recovery verifier、maintenance lock | 保障 | 高 | 自动猜测可能删除唯一完整代；无恢复则人工风险高 | incoming/original/abandoned 组合 |
| UPG-023 | 严格当前数据库：无 SQLx migration ledger | `verify_current_database` | 保障 | 中 | 开发期旧迁移历史可能被当前产品误接受 | `_sqlx_migrations` 存在即拒绝 |
| UPG-024 | Schema 规范 fingerprint | sqlite schema canonicalization | 保障 | 高 | 不同 DDL 可能冒充同 revision | 顺序、internal objects、trigger/index |
| UPG-025 | 备份目录安全解析 | openat2/dirfd、no symlink、单链接普通文件 | 保障 | 高 | 恢复可被路径替换或读取特殊文件 | symlink/hardlink/FIFO/device/race |
| UPG-026 | 资源大小、条目、manifest 上限 | current/sqlite/manifest constants | 保障 | 中 | 恶意或损坏备份可耗尽内存/磁盘/时间 | 边界值与 overflow |
| UPG-027 | no-replace 输出语义 | pending UUID、RenameFlags::NOREPLACE | 保障 | 中 | 备份命令可能覆盖唯一已有证据 | existing dir/file/symlink |
| UPG-028 | `RecoveryAction` 显式选择 | CLI enum、journal verifier | 保障 | 中 | 工具若自动选择可能提交未验证目标或删除原代 | 非法值、commit 前验证 |
| UPG-029 | 通用 future `UpgradeEdge` 数据模型 | support types，当前为空 | 可选 | 低 | 删除只影响未来扩展；当前命令和支持能力不变 | 序列化空数组 |
| UPG-030 | 当前 Schema fixture 仅用于测试 | `tests/fixtures/current/*.sql` | 开发运维 | 中 | SQLite backup 测试无法构造精确官方库 | fixture fingerprint 等于 code allowlist |
| UPG-031 | Rust fmt/clippy/test 门禁 | Cargo、CI | 开发运维 | 中 | 路径/恢复状态机回归可进入发行 | all-targets、warnings denied |
| UPG-032 | source-bound release、SBOM、support snapshot | `scripts/`、workflow | 开发运维 | 高 | 制品支持范围、来源和依赖不可证明 | clean tag/SHA、archive、tamper |
| UPG-033 | 中文学习、流程、功能和运维文档 | README、`docs/` | 开发运维 | 低 | 操作者可能把开发期“无升级边”误解成自动升级 | 链接、CLI help、支持矩阵抽查 |
| UPG-034 | Sentinel/Dufs 当前组合备份暂未实现 | support 中 current_state 为空 | 可选 | 高 | 目前不能宣称这两项目可由本工具完整备份；不得退回 generic SQLite 伪装支持 | support JSON 与 CLI 均无命令 |
| UPG-035 | 所有历史升级边暂未实现 | `upgrade_edges=[]`，无 SQL/adapter/CLI | 核心 | 高 | 用户必须重新部署开发数据；这是当前有意边界，不得手工改库冒充升级 | 源码/CLI/docs 无历史 edge |

## 3. 当前支持矩阵

| 产品 | 当前 backup/verify/restore | recover | 历史升级边 | 外部要求 |
|---|---|---|---|---|
| Media Backup `0.2.0` | DB + data tree 组合状态 | commit/rollback | 无 | 无 |
| Host Monitoring `0.7.0` | SQLite | commit/rollback | 无 | 无 |
| Sunshine Manager `0.7.0` | SQLite + 密文认证 | restore 中断恢复当前不对外声明 | 无 | credentials key ID + 32-byte key |
| Sentinel Monitor | 未实现 | 未实现 | 无 | 未来必须组合 DB/config/contract/recordings/key |
| Dufs RAM | 未实现 | 未实现 | 无 | 未来必须组合 DB/config/shared tree/owner domain |
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
- 自动停止或启动 systemd/launchd/Windows service；
- 任意 SQLite 文件备份；
- 把 credentials key 放入备份；
- 跨文件系统的非原子安装；
- Sentinel 或 Dufs 的 generic SQLite-only 恢复；
- 自动清理无法判定的 recovery 证据；
- 宽松接受未知 manifest 字段或资源。

## 7. 功能完成定义

代码存在一个复制函数不代表功能完成。新功能只有在支持矩阵、严格 manifest、路径与文件身份、锁顺序、容量预算、snapshot、target 验证、external key、journal、故障注入、commit/rollback、CLI、release provenance、SBOM 和中文演练文档全部完成后，才可列为支持。
