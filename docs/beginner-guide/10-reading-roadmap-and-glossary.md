# 10. 源码路线、演练与术语表

## 10.1 阅读路线

先读 CLI/support/catalog/current identity，再读 manifest/path/locking/SQLite clone，随后读 restore/journal，
最后逐产品 adapter 与 release scripts。不要从某个命令示例反推通用安全模型。

推荐分四轮阅读，每轮都带一个明确问题：

1. **可达性**：读 `src/main.rs::Command`、`src/support.rs`、`src/catalog.rs`，判断什么能做、什么只是资源知识。
2. **输入证明**：读 `src/manifest.rs`、`src/sqlite.rs` 的安全路径/Schema/key 逻辑，判断不可信输入如何拒绝。
3. **持久状态机**：读 `src/current.rs` 与 `src/sqlite/restore.rs`，逐个标记 fsync、rename、journal phase。
4. **交付证明**：读测试、workflow 与 `scripts/`，判断源码、依赖、binary、support snapshot 如何绑定。

每轮都先画成功路径，再为每个节点写一个失败输入和磁盘状态。只读函数名而不追踪文件系统事实，很容易
漏掉恢复工具最重要的 crash boundary。

## 10.2 按问题找入口

| 问题 | 入口 |
|---|---|
| 命令是否支持 | support/catalog/CLI |
| source 被拒绝 | current identity、adapter validator |
| backup 失败 | snapshot、inventory、manifest、fsync |
| restore 中断 | journal、stage、recover command |
| tree 超限 | budgets、inventory/walker |
| key 失败 | credential requirement/envelope verifier |
| release 验证 | release metadata、checksum/signature scripts |

更精确的源码导航如下：

| 事实 | 代码锚点 | 阅读时核对 |
|---|---|---|
| 六产品 canonical slug | `src/catalog.rs::Product` | 只接受当前名字，无 alias |
| 实际 current 能力 | `src/support.rs::support_matrix` | Media/Host/Sunshine 差异；edge 全空 |
| CLI 参数组合 | `src/main.rs::Command`、`sqlite_credentials` | key 只属于 Sunshine；recover 只到 Host/Media |
| Foundation manifest 策略 | `src/manifest.rs::BackupManifest` | strict shared parser + 产品级规则 |
| SQLite official identity | `src/sqlite.rs::official_sqlite_identity` | Host/Sunshine 唯一 SHA |
| Schema 重算 | `verify_schema_identity_database` | integrity/FK/metadata/canonical fingerprint |
| SQLite backup | `create_sqlite_backup_internal` | shared lock、online snapshot、publish、复验 |
| SQLite restore | `src/sqlite/restore.rs` | sidecar、journal、commit/rollback |
| Media backup/restore | `src/current.rs` | DB/tree 交叉验证和组合切换 |
| 发行 | `scripts/stage-release.sh`、`finalize-release.sh` | source-bound、no overwrite、签名/provenance |

## 10.3 最小演练集

1. current backup/verify 与单字节篡改拒绝。
2. no-clobber output/target。
3. WAL 有提交页的 SQLite snapshot。
4. symlink/hardlink/special file/path traversal 拒绝负例。
5. restore 每阶段中断的 commit/rollback。
6. external key 正确/错误/权限不安全。
7. Media v3 tree 的 root mode、非根 path/mode、文件 path/mode/size/SHA 与 budgets；确认
   hardlink/symlink/special file 被拒绝，并明确
   sparse/xattr/ACL 不在当前保真合同内。
8. target 产品 offline doctor 与启动 smoke。

每个演练保存“预期”和“实际”，并检查失败后 source/output/target 是否保持允许状态。最小集合还应加入：

9. `support` 与 `catalog` 对 Sentinel/Dufs 分别证明“能力可达”和“资源完整性”。
10. `inspect-manifest` parse 成功但资源篡改仍被 full verify 发现。
11. wrong product/version/schema SHA 与 extra SQLite object 拒绝。
12. Sunshine wrong key ID、wrong bytes、unsafe key file 和密文篡改。
13. Media DB 引用文件 missing/size/BLAKE3 不匹配。
14. 证明 CLI 无 `upgrade-*`，所有 `upgrade_edges=[]`。

## 10.4 术语

| 术语 | 含义 |
|---|---|
| generation | 逻辑一致的一代数据库及相关资源 |
| adapter | 一个精确 source/target 或 current 合同实现 |
| edge | 精确历史版本之间的有向转换 |
| manifest | 备份资源、身份、Hash 与要求的严格合同 |
| code allowlist | 编译进 binary 的受支持身份集合 |
| sidecar | SQLite WAL/SHM/journal 等伴随文件 |
| stage | 同文件系统、验证后才安装的私有来件 |
| no-clobber | 目标存在就拒绝，不覆盖 |
| recovery journal | 记录持久切换阶段和原件/来件身份的证据 |
| preserved original | 切换时保全的原始状态代 |
| external key | 不进入备份、用于认证/解密持久 Secret 的密钥 |
| tree inventory | 对目录每项语义和聚合身份的完整描述 |
| fsync | 要求文件/目录变更进入持久存储边界 |

| 术语 | 含义 |
|---|---|
| current identity | binary 内唯一允许的 product/version/revision/schema SHA 组合 |
| composite state | 必须作为一代共同备份/恢复的 DB、tree、config、companion 等闭包 |
| catalog | 产品完整资源知识；不是 adapter 支持声明 |
| support matrix | 正在执行的 binary 实际公开能力的机器可读 allowlist |
| full verify | 重读实际资源并复算 Hash/Schema/业务不变量/key 认证，不只是 parse manifest |
| pending output | 正式发布前、由工具私有创建的未完成备份目录 |
| incoming | 已构建并验证、等待安装的目标 generation |
| commit | 证明 incoming 后完成安装和证据清理的显式决定 |
| rollback | 保全/移开 incoming 并恢复 preserved original 的显式决定 |
| external requirement | manifest 中非秘密的 key ID/hash/算法要求；不是 raw key |
| code-owned fact | 来自 binary allowlist/产品当前代码而非输入自报的事实 |
| TOCTOU | 检查对象后、使用对象前被替换造成的竞态 |
| `EXDEV` | 跨文件系统 rename 失败；本工具不以 copy fallback 模拟原子提交 |
| offline doctor | 目标产品提供的停机业务状态验证；不由 Upgrade 通用层替代 |

## 10.5 容易混淆的词

| 不准确说法 | 应改成 | 原因 |
|---|---|---|
| “支持升级 Media/Host/Sunshine” | “支持它们的 current backup/verify/restore” | 当前无任何 historical edge |
| “catalog 支持 Sentinel/Dufs” | “catalog 描述资源，support 声明当前 adapter” | 资源知识不是命令能力 |
| “manifest 校验通过” | 区分 parse 与 full verify | `inspect-manifest` 不读资源 |
| “generic SQLite backup” | “仅 Host/Sunshine 的 SQLite-only adapter” | generic 机制仍有产品 allowlist |
| “restore 覆盖目标” | “保全 original 后安装 incoming” | `--replace-existing` 不直接 overwrite |
| “自动恢复” | “显式 commit/rollback” | 操作者负责业务选择 |
| “保存了 tree” | 列出当前 path/type/mode/size/SHA 边界 | 不承诺 xattr/ACL/sparse/hardlink |
| “有 journal 所以都能 recover” | 以 support 为准 | Sunshine 未公开 recover |

## 10.6 学成标准

能在不看命令示例时画出 backup/restore 时序；能解释每个 fsync/journal 的原因；能判断何时必须停止而非
猜 adapter；能独立演练中断恢复；能证明 raw key 未进入任何输出。

还应能完成一次代码评审：从 support/CLI 证明能力可达性，从 catalog 证明资源闭包，从 verifier 证明输入身份，
从 restore 状态机证明 crash 后磁盘可解释，从 release 脚本证明执行 binary 可追溯；最后指出所有明确未覆盖
的产品、资源语义、平台和历史版本。

## 10.7 自测题

1. 为什么 `inspect-manifest` 成功不能证明 SQLite backup 可恢复？
2. 为什么 Dufs catalog 中有 SQLite，却必须被 `backup-sqlite` 拒绝？
3. Host 与 Sunshine 使用相似 restore 原语，为什么 recover support 不同？
4. Media tree 中一个 hardlink 为什么不是“两个普通文件内容相同”的小问题？
5. journal 写入和第一次 target rename 之间掉电时，需要哪些持久事实？
6. 为什么额外 index 无需列入旧对象黑名单也会被拒绝？
7. `UpgradeEdge` 类型、未来准入章节、通用 stage 三者为何都不构成 migration engine？
8. 哪些信息可以写入 Sunshine manifest，哪些绝不能写？
9. 为什么正式支持写成 Linux AMD64 GNU，而不能写“Linux”？
10. 一个新 product enum entry 还缺哪些部分才是可发布 adapter？

## 10.8 深入文档

完整端到端阶段见[工作流程](../project-workflow.md)，产品/edge/能力矩阵见[功能与取舍](../feature-inventory-and-tradeoffs.md)，
生产命令、保管和事件处置见[运维文档](../operations.md)。
