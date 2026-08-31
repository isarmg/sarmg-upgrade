# 07. 为什么当前没有历史升级 Edge

## 7.1 先区分“未来设计”与“当前功能”

历史 edge 是一个精确 source version 到精确 target version 的有向转换。`sarmg-upgrade 0.2.0` 当前没有
任何 edge：`support --json` 的数组为空，CLI 不注册 `upgrade-*`，源码也没有旧 Schema SQL/parser。
本章后半解释未来准入标准，不表示命令已经存在。

仓库当前保留的 `UpgradeEdge { from, to }` 只是 support 输出的数据结构，六个产品实例全部是空 vector。它
没有注册 adapter、没有执行 trait、没有 graph，也不会因为填写一个结构体就产生可运行转换。评审时必须
把“能描述”与“能安全执行”分开。

## 7.2 当前不存在的组成件

一个真正历史升级闭包至少需要以下组成件；当前全部不存在：

| 组成件 | 当前状态 | 缺失时为什么不能宣称支持 |
|---|---|---|
| exact source identity allowlist | 无历史版本 | 无法证明输入究竟是哪一代 |
| raw source generation snapshot/parser | 无 | 可能被 current driver 改写或遗漏 WAL/资源 |
| source-backup manifest 与 verifier | 无 | 转换失败后没有可独立验证的原始证据 |
| product-specific transform | 无 SQL/代码 | 不存在字段、密文、文件语义转换 |
| target-from-zero builder | 无 | 不能保证只产生 current Schema |
| edge registry / graph selection | 无 | 没有受支持 from/to 路由 |
| `upgrade-*` CLI | 无 | 操作者没有被审核的执行入口 |
| upgrade recovery journal | 无 | current restore journal 不能代表转换过程 |
| historical fixtures/fault matrix | 无 | 无法证明旧输入、恶意输入与 crash 行为 |

可复用的 hash、backup、stage、restore journal 函数只能覆盖其中少量底层机制，不能补齐产品转换语义。

## 7.3 为什么开发期删除 edge

开发期 Schema、密文、目录和业务不变量仍可能快速变化。为每个试验版本保留 adapter，会让团队误以为
这些格式已经承诺长期迁移，并迫使测试覆盖无实际用户价值的旧分支。更严重的是，旧 parser 和宽松转换
会成为长期攻击面。因此开发数据默认重新部署；当前没有已支持 edge 时，即使数据不可重建，也不能用
current restore、临时 SQL 或外部脚本冒充本工具已支持迁移。未来产品版本稳定后若确有长期迁移需求，
精确 edge 只在本 `sarmg-upgrade` 仓库中以独立审核的 adapter、fixture、CLI 与 release 原子加入。

删除旧兼容不是“以后再试旧 parser”的延迟策略，而是 current-only 产品边界：不保留旧 DDL、migration
ledger 特判、serde alias、旧 product slug、双 fingerprint 算法、旧命令 alias 或环境变量 fallback。这样每个
正式版本只测试一个世界，安全审查也不需要证明所有历史宽松路径都不会绕过当前验证。

## 7.4 如何证明“确实没有”

```bash
sarmg-upgrade support --json
sarmg-upgrade --help
rg 'upgrade-|from-version|to-version' src
```

第一条是机器事实；第二条验证没有命令入口；第三条是开发审查辅助。文档、README 或 catalog 中出现产品
名称，都不能替代 support。

还应检查 `support --json` 中每个 product 的 `upgrade_edges`，不能只确认顶层没有某个 capability；检查 CLI
枚举没有 from/to 参数；检查 release 附带的 support snapshot 与实际 binary 输出一致。`rg` 可能命中文档中
的未来准入说明或 `UpgradeEdge` 类型，这些文本本身不构成功能，必须继续判断是否存在可达执行路径。

## 7.5 当前数据库如何处理额外对象

当前产品和本工具只接受 code-owned 当前 Schema identity，但不会按 `_sqlx_migrations` 或任何其他表名编写
历史特判。canonical fingerprint 对全部纳入范围的 `sqlite_schema` 行执行同一排序与 framing；额外表、索引、
trigger，缺失对象或 DDL 变化都会自然产生不同摘要，并在任何备份发布或恢复 mutation 前拒绝。这样既能阻止
“表看起来相似”的开发数据库混入当前备份，也不会为了某个旧实现留下特殊兼容代码。

共享 `sarmg-contracts =0.3.0` 和 `sarmg-schema-identity =0.3.0` 同样只描述当前协议；两者只从不可变
Git rev `1fe326081cfd896f05ff502e80f99504797c14c6` 取得。精确依赖的意义是让各项目对当前 manifest、metadata
和 fingerprint 使用同一事实，并不意味着 Foundation 能读取 0.2 或 0.1 数据库。不得改用 workspace sibling、
Cargo path dependency、可变 branch 或本地旧类型，也不得加入 serde alias、双算法比对或“先新后旧”parser；
未来历史输入只能由本仓库中精确绑定 source/target 的独立 edge adapter 处理，不能扩宽 current parser。

“无旧特判”也意味着不能把已知旧 SHA 加到 current allowlist 作为第二选择。current official identity 对每个
产品只有一个 version/revision/SHA；Schema 变化应作为全新 current 合同同步业务仓库、fixture、Upgrade
allowlist、文档与发布，不在同一 binary 中继续接受旧值。

## 7.6 未来 edge 的 source 证明

若首个稳定版本之后确需升级，adapter 必须验证 metadata、规范 Schema SHA、精确 migration ledger/checksum、
资源合同、密文和业务不变量。任何字段不符就拒绝，不能选择“最接近”的 adapter。

## 7.7 Source backup first

未来 adapter 必须先从受停机锁保护的 raw clone 建立 immutable source backup。只有 source backup 完整发布
并可独立 verify，才可从 target current SQL/code 从零构建新代；禁止在 source 原件上逐步 `ALTER TABLE`。

## 7.8 密文和组合资源

external key 只能在运行时提供，raw bytes 不进入包。转换必须先认证 source，再按 target 唯一算法/AAD
生成并全量验证。Sentinel、Dufs 等组合产品还必须把 config、companion、recordings/shared tree、owner、
mode、link 和容量预算纳入同一 journal，不能只升级 SQLite。

## 7.9 Target 与切换证明

target 必须通过和目标产品相同的 code-owned metadata/Schema/业务不变量、全部密文和资源 inventory 检查。
随后才可创建 durable recovery journal，保存原代，并在同文件系统原子安装。每个持久边界都要有 kill/
error 故障注入以及 commit/rollback 测试。

## 7.10 遇到旧开发数据怎么办

停止操作并保留只读副本。不要手改 metadata/SHA，不要复制旧 SQL 回本仓，不要把 generic backup 当迁移。
优先重新部署并从业务源重新导入。当前 `upgrade_edges=[]` 时，即使数据有不可重建价值，也应保全只读副本
并停止，等待明确产品决策；不得临时手改或假借另一工具。未来产品版本稳定、团队决定长期支持该转换后，
必须在本 `sarmg-upgrade` 仓库中完整实现并公开精确 edge；产品 runtime 和 current adapter 始终不增加兼容
分支。

## 7.11 本仓库未来历史 Edge 的最小边界

未来稳定版本若旧数据确有不可重建价值，`sarmg-upgrade` 中新增的每条独立 edge 至少应固定：

- 唯一 source product/version/revision/SHA 与完整资源闭包；
- 唯一 target current identity，不允许“最新”动态选择；
- 原始 source immutable backup、验证器、保管人和恢复演练；
- 转换代码、fixture、恶意输入与每阶段 crash 测试；
- raw key 的独立输入、认证、轮换与零日志约束；
- source-bound 发行 binary SHA、审核者、允许运行的资产清单；
- edge 支持期限，以及源码、制品、Secret 与数据证据的保留或销毁方案。

edge adapter 不得成为产品运行时依赖，也不得把转换结果的旧字段继续带到 target。它必须与 current adapter
分模块、分 CLI、分 fixture，且只能接受唯一 source/target identity；不能通过 shared fallback 令 current
backup/restore 兼容旧输入。缺少任一闭包时，edge 必须继续不出现在 `support --json`。

## 7.12 未来准入标准不是路线承诺

本章 7.6 至 7.9 只说明“若要实现，最低需要什么”，不承诺一定实现、不指定版本，也不表示已经拥有通用
升级 engine。任何计划文档、issue 或空 struct 都不能改变当前事实；只有代码、CLI、support、fixture、
故障矩阵、运维与 release 全部落地后，才允许某个精确 edge 出现在 `support --json`。

## 7.13 本章检查

应能列出真正历史 edge 的完整闭包，解释 current restore journal 为什么不是 upgrade journal；能说明额外
SQLite 对象如何由统一 fingerprint 自然拒绝而无需旧表黑名单；能给出旧开发数据的标准处置；并能证明
`UpgradeEdge` 类型存在、catalog 有产品资源和文档描述未来标准都不代表当前支持升级。
