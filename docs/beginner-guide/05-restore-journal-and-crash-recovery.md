# 05. 恢复、Journal 与中断处置

## 5.1 恢复前验证

先严格验证 backup、全部资源、code identity 和 external key，再取得排他锁并验证目标策略。`--replace-
existing` 是明确授权，不表示可跳过 preserved original 或路径检查。

恢复前置条件至少包括：binary/support 与备份合同一致；备份完整 verify；产品服务、companion、watchdog
已停；目标父目录可信且有足够空间/inode；目标和 stage 能使用同文件系统 rename；external key 可用；
目标为空或是精确可验证的 current generation。旧版本目标不是 `--replace-existing` 可接受的输入，因为当前
工具没有历史转换能力。

`--expect-version` 用于 SQLite restore/recover，以及 Media recover 的 current 版本二次确认。Media 首次
`restore-media` 已由输入 manifest 精确限定 current version，因此没有该参数。任何位置的
`--expect-version` 都不会选择迁移路径，也不会让 `0.6.0 -> 0.7.0` 发生转换。

## 5.2 Stage

在目标相邻同文件系统建立私有 stage，复制/生成来件，设置最终 mode/owner，重新计算 Hash/Schema/tree 并
同步。目标在 stage 完整前不变。

stage 不是可供产品启动的半成品位置。操作者不得把服务配置临时指向 incoming，也不得在 stage 内手工修
数据。工具会重新验证 stage 的 bytes/SHA/Schema/业务不变量；只有通过后才进入 journal 所描述的安装流程。

## 5.3 Recovery journal

journal 记录工具/产品/版本/adapter/Schema identity/时间、source backup 的规范路径与 inode/path identity、
manifest version/time/bytes/SHA、source tree identity、database/tree 目标与父路径 identity、由同 nonce 推导的
original/incoming sibling 名称、incoming 与 optional original 的 DB/tree 完整内容 inventory、阶段和预期 Hash。
它先于第一次目标 mutation 持久化，并在每阶段更新后同步目录。Media current journal 最大 1 MiB、唯一版本
为 2；不读取旧 v1 journal，也不给缺失字段补默认值。`configuration` 和 `external_requirements` 必须精确为 `[]`。

journal 的安全意义有三层：

- 身份：绑定 product/version、目标、incoming、original 和预期摘要；
- 顺序：表明哪些持久操作被允许发生，以及上一次已同步 phase；
- 恢复授权：recover 只能在磁盘事实与 journal 允许的状态组合相符时继续。

它不是普通进度日志。编辑 JSON 让 phase “看起来正确”、从另一台机器复制 journal、或只保留 journal 而
移动 original/incoming，都会破坏证明链。

Media 的 source/recovery/database/tree 必须是显式 canonical absolute path，source 与 targets 必须 disjoint；
recovery 只能是 database 同级 `.<db>.recovery-<32位小写十六进制 simple UUID>`，stage/original 必须由同一
nonce 精确推导。recover 比对六项 CLI 输入后，取得 database/tree 两把 sibling non-blocking exclusive 锁，
再验证全部 source/manifest/stage/target/original 证据。锁内 pending journal 属于未提交更新，会被丢弃；
已持久化 `rollback-started` 后不能改选 commit。重复相同 action 可幂等推进，但每次 cleanup 前仍会重验证证据。

Media v2 journal 精确绑定 Cargo tool version，但不内嵌 release binary SHA。工具会拒绝不同 tool version；
同版本制品是否真是同一受信 bytes，仍必须由操作者用变更单、签名和 binary SHA 证明，不能从 journal 推断。

## 5.4 安装阶段

```text
prepared -> original preserved -> incoming installed -> installed verified -> committed
```

通用组合结构可表达多个资源，但当前 Media 只允许数据库和树，configuration 必须为空。任意崩溃后不能仅从
“目标存在”推断完成。

SQLite restore journal version 1 的 phase 名是 `phase-prepared`、`phase-originals-preserved`、
`phase-installed`、`phase-verified`；Media composite journal 的唯一 current version 2 使用 `prepared`、
`originals-preserved`、`installed`、`verified`，并在回退中持久化 `rollback-started`、
`rollback-verified`。两种 journal 不是通用互换格式，recover 命令也不能交叉；Media v1 不保留兼容 reader。

“installed verified”仍不等于清理完成。original 与 journal 可能继续存在，直到父目录同步、最终验证和
commit cleanup 全部完成。发现残留时要从 support 选择合法 recover，而不是仅看目标可打开就删除证据。

## 5.5 Commit

重新验证 incoming/installed 是精确 target，必要时认证密文，确认所有组件一致，完成目录同步，再删除
preserved original 和 journal。无法证明目标正确时 commit 必须拒绝。

commit 的前提是操作者选择保留 incoming 作为正式目标。工具仍需把磁盘当前位置与 journal hash 对上，
重新运行 current identity/产品验证；Sunshine 还需要 external key 认证，但当前没有公开 Sunshine recover，
因此不能通过 CLI 进入这一通用描述的路径。

## 5.6 Rollback

验证 preserved original 与 journal identity，再原子恢复原件并同步。SQLite recovery 会把已安装来件保存为
`abandoned-new` 证据；Media recovery 只允许精确 incoming，在恢复原代时把它暂移回已绑定 stage，并在
`rollback-verified` 后随 recovery cleanup 删除。两者都恢复原始字节，不把原件重新解释成另一版本；本工具
也没有“之后再升级”的命令。

本仓当前只允许替换精确 current 目标，所以正常 rollback 后 original 也应是 current；“rollback 可恢复任意
旧版本”不是支持合同。对原先不存在的新目标，rollback 的语义是移除/保全已安装 incoming 并恢复为
“目标不存在”，而不是凭空生成 original。

## 5.7 操作者中断流程

保持所有服务停止；保存错误和 binary SHA；不移动/编辑 recovery，也不移动、替换或重新封装原 source
backup；修复空间/挂载/key 等环境问题；用
完全相同产品、版本、路径和身份运行对应 `recover-* --action commit|rollback`。Media 命令不会从 journal
替操作者猜上下文，必须重新显式提供 `--expect-version 0.2.0`、原 `--input`、原 `--database`、原
`--data-dir` 与错误报告中的 `--recovery`；工具再逐项比对并取得 DB/tree 两把排他锁。

建议按以下顺序记录与决策：

1. 冻结产品服务、companion、watchdog 和所有自动重试任务。
2. 保存完整命令、退出码、stderr、binary SHA、`support --json` 与 recovery 路径。
3. 只读记录 recovery、原 source backup 与目标父目录的 mount、owner/mode、inode/容量；不要遍历并修改内容。
4. 判断是环境可恢复问题（空间、只读挂载、权限）还是 identity/journal 不一致的安全事件。
5. 对照业务目标选择 commit 或 rollback；不要让 shell 脚本默认选择。
6. 仅调用 support 明确列出的对应 recover；失败后停止并升级事件，不叠加手工动作。
7. 完成后运行 backup verify、产品 offline doctor、隔离 smoke，并记录 recovery 已由工具清理。

## 5.8 禁止手工拼接

不要把 stage 文件复制到目标、删除 journal、重命名 preserved 目录或编辑阶段值。手工动作会破坏工具
用于证明的身份，使后续安全恢复不可判定。

也不要通过复制 backup 的 `database.sqlite3` 到生产目标来“绕过 journal”，不要预先删除目标 `-wal/-shm/
-journal`，不要把 Media database 和 tree 分两次替换。这些动作可能看似让文件就位，却失去 generation
一致性和 rollback 能力。

## 5.9 测试点

在每个 rename/fsync 前后故障注入；验证重复 recover 幂等、错误 action/路径/key 拒绝、journal 篡改拒绝、
跨设备拒绝和最终无临时残留。

## 5.10 当前 recover 支持矩阵

| 操作对象 | recover 命令 | 当前允许 | 不能做什么 |
|---|---|---|---|
| Media restore | `recover-media-restore --expect-version 0.2.0 --input BACKUP --database DB --data-dir TREE --recovery RECOVERY --action commit\|rollback` | `commit` / `rollback` | 六项参数都必填且路径必须与 journal 精确一致；不接受 Sentinel/Dufs，不拆分 DB/tree |
| Host SQLite restore | `recover-sqlite --product host-monitoring --expect-version 0.7.0` | `commit` / `rollback` | 不接受其他 product/version |
| Sunshine SQLite restore | 无 | 事件保全与人工升级 | 不得假装 Host、不得缺 key 续接 |
| Sentinel/Dufs composite restore | `recover-current --product PRODUCT --expect-version VERSION --input BACKUP --database DB --data-dir TREE --recovery RECOVERY --action commit\|rollback` | `commit` / `rollback` | product/version/path/key 必须与 journal 精确一致 |
| historical upgrade | 无 edge/命令 | 不适用 | 不得把 restore recovery 称为 upgrade recovery |

## 5.11 常见状态判断错误

| 观察 | 错误推论 | 正确处理 |
|---|---|---|
| target 存在且可打开 | “已经提交，可以删 original” | journal 可能尚未 verified/cleanup；用对应 recover |
| target 不存在 | “恢复尚未开始” | original 可能已 preserve、incoming 待 install；查看原错误并保全 recovery |
| original 存在 | “一定应 rollback” | incoming 可能已验证，应由业务决策 commit/rollback |
| journal phase 较旧 | “rename 一定没发生” | 崩溃可能发生在 rename 与 phase fsync 之间；让 verifier 对照磁盘身份 |
| Sunshine 有底层 journal | “可以调用 Host recover” | support 未公开且 Host 路径不认证 Sunshine key；进入事件处置 |

## 5.12 本章检查

应能画出 stage → journal → preserve → install → verify → cleanup，并指出第一次 mutation 前必须有哪些已
持久事实；能说明 commit/rollback 是业务选择而不是成功/失败别名；能列出 Media、Host、Sunshine recover
能力差异；能解释为什么手工编辑 journal 或删除 sidecar 会使安全恢复不可判定。
