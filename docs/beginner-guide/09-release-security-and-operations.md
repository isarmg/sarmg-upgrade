# 09. 正式发行、安全与生产运维

## 9.1 发行信任

从干净 annotated tag 构建 source-bound binary，运行完整门禁，暂存 capability catalog、release metadata、
CycloneDX SBOM、环境和 provenance。publish job 不 checkout source，以 Ed25519 签名 checksum。

正式发布只产生 `x86_64-unknown-linux-gnu` 制品。stage 阶段必须把 binary、source identity、support/catalog
snapshot、SBOM、构建环境和 provenance 绑定在一起；finalize/publish 只消费已暂存且已验证的内容，不能在
发布 job 重新 checkout 一个可能漂移的源码树。annotated tag、release 与 asset 都不覆盖。

Foundation 两个 crate 均精确固定为 `=0.3.0`，Git rev 固定为
`1fe326081cfd896f05ff502e80f99504797c14c6`，它们是不可变供应链输入。不得为了构建成功放宽 semver、
切到 branch HEAD、workspace sibling、Cargo path dependency、复制旧源码或启用本地 fallback；否则同一
Upgrade 版本可能产生不同线协议。

## 9.2 操作者验签

在触碰生产前，用独立可信渠道得到 public key，验证 `SHA256SUMS` 签名、outer digest、archive checksum，
解包后再次核对 binary identity/support/catalog。不要直接执行下载目录里未验制品。

验证至少覆盖两层摘要：归档/outer digest 证明运输对象，解包后 binary SHA 与 release metadata 证明实际执行
文件。public key 必须通过独立可信渠道获取，不能从同一未验证下载目录拿 key 和签名。保存验签输出到变更
单，但不要让日志包含 Secret 或生产 manifest 私有路径。

即使签名正确，也要确认 support：签名证明“谁发布了这些 bytes”，不证明该 binary 包含你希望的产品能力。
formal target、tool version、product/current operation 与空 `upgrade_edges` 都要核对。

## 9.3 变更窗口

记录服务/companion 停机与 watchdog 禁用、路径/uid/gid、产品、唯一 current version、具体 operation、key ID、
空间/inode/文件系统、命令、binary SHA、backup destination、commit/rollback 判断点。当前没有 from/to 参数或
历史 edge，不应在 current 操作单中伪造这两个字段。记录不含 raw key。

空间预算不能只看源 DB 大小。Media/replace 最坏需同时容纳 backup、incoming DB/tree、preserved original、
manifest 与 recovery；SQLite 目标还可能有 main + sidecars。记录源/目标文件系统、free bytes、free inode 和
stage 是否同 mount。先在隔离环境测得实际峰值与 RTO，再设置窗口。

停机证据应包含 service/companion 状态和禁止自动拉起的措施；“没有看到进程”不足以排除 watchdog、timer、
容器编排或另一操作者启动。工具的 maintenance lock 是第二道保护，不替代变更协调。

## 9.4 执行原则

当前流程只写成先 current backup/verify，再 current restore；不存在可执行的 historical upgrade。持续监控
空间与日志；错误立即停止，不追加手工动作。工具不自动启动服务，完成后先 offline doctor，再启服务和
业务 smoke。

若输入是非 current 数据，应停止；当前无已支持 edge 时只能重建，不能把 restore 当升级。未来稳定版本若
本 `sarmg-upgrade` 仓库已通过 `support --json` 精确公开对应 source/target edge，才可按该独立 adapter/CLI
流程执行。若 restore 报告 recovery，则变更窗口转入恢复事件，后续动作由产品 support 与 journal 决定，
不能从头重跑。

## 9.5 备份保管

不可变、最小权限、静态加密、异地 3-2-1；external key 在独立 Secret 系统。记录 retention、访问审计与
可验证销毁。定期从真实介质恢复，不只运行 checksum。

工具 output 内部是 exact set；不要让备份软件在目录中加入说明、sidecar 或修改 mode。需要额外 metadata 时，
在外层对象/目录保存，并分别记录其 checksum。不可变副本至少一份与生产权限域隔离；Sunshine key 使用独立
Secret 系统、审批链和丢失告警，避免数据与 key 单点同时失守。

“verify 通过”是某一时刻的证明。介质 bit rot、权限漂移、key 轮换、release 丢失或产品 doctor 变化都可能
影响可恢复性，所以要从真实异地介质定期执行全流程 restore/doctor/smoke，并记录 RTO。

## 9.6 Recovery 事件

发现 recovery path 时冻结服务，保全 binary/log/journal/目录身份，修复环境后明确 commit/rollback。交接
时必须传递全部参数和当前阶段，不能让下一班“从头再跑”。

交接最小包包括 binary/archive SHA、签名结果、support JSON、原命令与退出码、完整错误链、recovery path、
目标绝对路径、mount/空间、service 停止证据、key ID/Secret version，以及 commit/rollback 决策责任人。
raw key、数据库内容和 journal 全文不进入普通工单；敏感证据存放在受控位置。

Sunshine restore 残留是特殊边界：support 不公开 recover，因此保持停服和证据，升级到人工安全事件；不能
用 Host recovery，不能手工补 key 参数调用内部库。

## 9.7 安全事件

停止工具和产品，隔离可能泄露的数据库、配置、媒体/录像/共享树与 key，保全 release/SBOM/provenance/
日志摘要，轮换 external key 并按 adapter 重新加密。公开报告不包含私有路径或 manifest 敏感内容。

轮换 Sunshine key 不能只替换 Secret 文件；产品持久密文必须按产品 current 流程重新加密并全量认证，然后
重新生成备份。Upgrade 工具验证给定 key 与现有密文，不负责在线 key rotation。

## 9.8 监控发行

关注 workflow 权限/action pin、异常 tag/release、签名 key、制品覆盖尝试和支持目录漂移。已发布 asset
不得覆盖；问题使用新版本修复。

还应监控 Foundation 依赖 revision、Cargo.lock 第二版本、正式 target 漂移、support snapshot 中意外出现
历史 edge、CLI 新增未文档命令、Sentinel/Dufs 被误列 current，以及 release archive 内存在非 AMD64 GNU
制品。这些都是边界变化，不是普通文档更新。

## 9.9 完成条件

backup 已 verify；target 通过工具与产品 doctor；服务 smoke 成功；无未解释 recovery/pending stage；
变更记录含所有 Hash 和判断；原备份/key 的保管策略已确认。

## 9.10 生产前逐项 Go/No-Go

| 检查 | Go 条件 | No-Go 例子 |
|---|---|---|
| 平台 | `x86_64` + GNU，formal target 精确 | ARM、musl、未知 libc |
| 制品 | tag/signature/checksum/binary SHA/provenance 全匹配 | 只验证文件名或下载 TLS |
| 能力 | support 精确列 product/current operation/version | 仅 catalog 出现、历史 edge 为空却要求升级 |
| 停机 | product/companion/watchdog 全部受控 | 仅口头确认服务已停 |
| 路径 | 绝对、可信父级、身份已记录、同文件系统 stage | 用户可写父目录、symlink、跨 mount |
| 容量 | bytes + inode 覆盖峰值并留安全余量 | 只够放一份 DB |
| backup | 新 output，成功退出并 full verify | 只有 manifest parse 成功 |
| key | Sunshine key ID/file/Secret 版本可验证 | key 只存在于单台生产机 |
| rollback | 对应产品 recover 能力和人工决策已演练 | Sunshine 却假定可自动 recover |

任一 No-Go 都应推迟变更，而不是临时放宽程序检查。

## 9.11 完成后的证据与清理

完成记录应包含：输入 backup 的 verify 结果、target current identity、产品 doctor/smoke、服务重启与最小读写、
无未解释 recovery/pending、实际 RTO/空间峰值、备份与 key 的保管位置、binary/support/provenance 摘要。

清理只发生在状态已证明并结案后。普通 retention job 不得处理 recovery；不要删除唯一输入 backup、唯一
preserved evidence 或唯一可取得的旧 key。实验数据也按敏感数据销毁，删除 manifest 不等于删除数据库/tree。

## 9.12 本章检查

应能解释签名、binary SHA 与 support 分别证明什么；能编写不含 raw key 的变更单；能计算 replace 峰值而
非只看 DB 大小；能区分 Media/Host/Sunshine 中断边界；能在非 current 输入出现时停止，而不是寻找隐藏的
upgrade 路径。
