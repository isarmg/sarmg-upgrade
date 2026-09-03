# 08. 测试、调试与新增 Adapter

## 8.1 基础门禁

```bash
python3 scripts/check-workflow-supply-chain.py
cargo +1.98.0 fmt --all -- --check
cargo +1.98.0 check --locked --all-targets --all-features
cargo +1.98.0 clippy --locked --all-targets --all-features -- -D warnings
cargo +1.98.0 test --locked
bash -n scripts/*.sh
git diff --check
```

这些是开发者在代码全部完成后的统一门禁；正式发行还要执行 supply-chain、stage/finalize 与 clean checkout
复建。本文列出命令不表示可以跳过 `--locked`、固定 Rust 版本或只运行改动附近的单元测试。路径安全和恢复
状态机的回归往往跨模块，至少需要 all-targets/all-features 覆盖。

当前 checkout 的正式门禁事实以 `.github/workflows/ci.yml`、`.github/workflows/release.yml` 和
`scripts/check-workflow-supply-chain.py` 为准；文档命令若与 workflow 不一致，应先查明漂移而不是挑更宽松
的一边。

## 8.2 Fixture 原则

当前 fixture 由对应产品的 current Schema 事实生成并固定 identity；仓库现有
`tests/fixtures/current/host-monitoring.sql` 与 `sunshine-manager.sql` 用于证明 code allowlist。当前没有历史
source fixture，这一点与 `upgrade_edges=[]` 一致。若未来真正增加历史 edge，source fixture 才必须来自该
精确历史发行事实，target 仍由 current target SQL/code 从零创建。

测试数据不得包含生产 Secret。恶意 fixture 覆盖 Schema 自报与实际不符、额外/缺失对象、corrupt SQLite、
foreign-key 错误、sidecar、链接、mode、路径穿越、duplicate/unknown manifest fields 和超预算。Sunshine
fixture 使用非生产 key，并验证错误 ID、错误 bytes、密文篡改和 malformed envelope。

fixture 更新不能只把断言改到“新 SHA”。正确顺序是：从业务产品唯一 current DDL 生成、用 Foundation
canonical 算法重算、核对 metadata、同步 Upgrade code allowlist 与 fixture、运行跨仓一致性门禁，再在文档
记录唯一值。旧 fixture 不保留为 fallback。

## 8.3 故障注入

在 copy、file sync、manifest、directory sync、stage、journal、preserve、install、verify、cleanup 每个持久
边界注入错误/kill。检查原件、backup、recovery 和重复执行的可解释性。

建议用状态矩阵记录每个故障点，而不是只统计测试数：

| 故障区间 | 必须保持的事实 | 允许的后续动作 |
|---|---|---|
| pending 创建前 | source/output/target 不变 | 修复参数后新 output 重跑 |
| 资源复制中 | 正式 output 不存在 | 安全识别 private pending 后处理 |
| manifest/fsync/publish | 既有 output 不覆盖 | 若未发布，使用新 output；已发布先 verify |
| journal 持久化前 | target 不变 | 重新 restore |
| preserve/install/verify 中 | recovery、original/incoming 可解释 | 对支持产品显式 commit/rollback |
| cleanup 中 | installed 已验证但证据可能残留 | 对支持产品 resume commit，不手删 |

故障应覆盖普通 error return 与进程 kill/掉电模型。只 mock 一个 Rust `Result::Err` 不能证明目录项同步与重启
后的 journal 恢复。

## 8.4 新 current Adapter 步骤

1. 从产品 current 代码固定 canonical slug、version、revision、schema SHA 和完整持久资源合同。
2. 判断能否使用 SQLite-only；只要还有 tree/config/recordings/companion 就必须专用 composite adapter。
3. 取得可信 current fixture，先实现只读 identity 与业务不变量 validator。
4. 明确服务/companion/Agent/Secret 哪些在 generation 内，定义 canonical lock 顺序。
5. 实现 pending + snapshot/copy + manifest-last + fsync + no-clobber backup。
6. 实现读取所有资源字节的 full verify，而不只 parse manifest。
7. 实现同文件系统 stage、durable journal、original preserve、install、installed verify。
8. 只有能安全验证所有阶段时才公开 recover；否则 support 明确为空并提供事件流程。
9. 加上链接/特殊文件/race/超限/错误 product/version/key/业务引用和 crash 负例。
10. 同步 CLI、support、catalog、release snapshot、功能取舍、流程、运维和初学者文档。

这是 current adapter，不涉及 from/to 或字段转换。若目标是历史 edge，必须另行满足第 7 章的 source backup、
historical validator、transform、target-from-zero 和 upgrade recovery 闭包，不能把两者混成一个“新 adapter”
清单。

## 8.5 调试顺序

先确认 binary/support，再查参数/path identity、锁、source identity、clone、manifest、target build、journal
阶段。不要边调试边修改生产原件。

一个可复现 bug report 应包含非秘密输入身份：commit/binary SHA、平台 target、完整 support、命令名与删去
Secret 的参数、文件系统类型、退出码、完整错误链、是否产生 recovery，以及最小合成 fixture。不要上传
生产数据库、媒体/录像树、key、包含私有绝对路径的 manifest/journal。

按“最早失败证明”定位比按错误字符串猜测更可靠：

1. support 是否授权操作；
2. CLI product/key 参数组合是否成立；
3. path/dirfd/owner/mode/nlink 是否成立；
4. maintenance lock 是否取得；
5. manifest/resource budget 是否成立；
6. SHA/SQLite integrity/FK/Schema 是否成立；
7. 产品业务 invariant/key 认证是否成立；
8. restore journal 与磁盘 generation 是否一致。

## 8.6 安全审查问题

不可信数据在哪首次解析？容量在哪里限制？路径如何锚定？哪一步首次 mutation？之前是否有 source backup？
掉电后 journal 是否足够？raw key 能否进入输出？错误是否会误删证据？

审查还应问：support 是否可能把计划当实现；catalog 是否被当授权；generic adapter 是否遗漏组合资源；
manifest 是否允许 unknown/duplicate/unsafe path；同一个 identity 是否由两套算法计算；Sunshine 是否只比较
key ID 而未认证密文；正式 target 是否被放宽；是否新增旧 alias/fallback；日志与 `Debug` 是否打印 Secret。

## 8.7 名称/合同变化

同步 crate/binary/package、support/catalog product slug、manifest、命令、recovery 名、脚本、SBOM/provenance、
测试与文档；删除旧 alias。全文和路径搜索只是开始，还需真实 release 解包验证。

Schema 变化还必须同步业务仓库 current DDL、`product_metadata`、Foundation golden vectors（若通用算法
变化）、Upgrade official identity/fixture、support/release snapshot 和三处 current 表格。旧 SHA 不保留为
current 候选；未来产品稳定后若需要历史转换，必须在本仓库另增精确 edge 的 adapter、source/target fixture、
CLI 与 release，不能扩宽 current allowlist。

## 8.8 提交标准

一个 adapter/重大安全边界一个提交；完整门禁和故障矩阵通过；无 fixture 漂移、Secret、target、临时 backup
或 recovery；文档命令由当前 CLI 帮助核对。

## 8.9 当前至少应覆盖的测试层次

| 层次 | 目的 | 代表性负例 |
|---|---|---|
| Foundation 合同单元 | 统一 JSON/schema framing | unknown field、safe integer、row order |
| manifest 包装单元 | 产品级策略 | wrong product、resource duplicate/unsorted、key requirement |
| SQLite adapter | snapshot 与 exact identity | WAL、corrupt/FK、extra DDL、wrong SHA |
| Media adapter | DB/tree 同代与业务引用 | missing blob、BLAKE3/size 错、symlink/hardlink、tree drift |
| Sunshine key | Secret 文件与密文认证 | mode/nlink/race、wrong key/ID、tampered envelope |
| restore/recover | crash consistency | 每个 phase、journal tamper、same-filesystem、重复 action |
| CLI/support | 可达能力边界 | Sentinel/Dufs generic SQLite 拒绝且 composite 可达、Sunshine recover 拒绝、无 upgrade 命令 |
| release | source-bound 制品 | dirty tree、SHA mismatch、asset overwrite、support snapshot drift |

## 8.10 本章检查

应能解释 current fixture 与未来 historical fixture 的区别；能为一个故障点写出掉电前后磁盘事实；能按
最早失败证明调试而不修改生产输入；能指出新增 current adapter 的完整闭包；能说明为什么只更新 SHA 断言
或只加入 catalog/support 一行都不算完成。
