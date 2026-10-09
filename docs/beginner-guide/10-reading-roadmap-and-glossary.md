# 10. 阅读顺序与术语

先从 CLI 的 plan→apply→inspect→recover 阅读，再沿 mod.rs 的 preflight/apply_inner/recover 和对应故障测试。接着读 snapshot 的 nofollow/inventory/ownership/restore，native_release 的 completeRoot/current/repair，execution 的属主与固定FD执行。

| 术语 | 在工具中的含义 |
|---|---|
| ReleaseIdentity | 真实软件、源码 full revision、target、状态合同摘要 |
| SchemaIdentity | 实际持久结构身份；独立于软件版本 |
| state_paths | 产品全部不可重建的持久输入路径 |
| artifact | 本工具受签名的交付方式与 physical Root 定义 |
| pending | 阻止正常启动的维护门；存在本身不授权写入 |
| maintenance lock | 工具排他、runtime共享的运行权协议 |
| inventory | 每项路径、类型、模式、长度和hash |
| ownership | 每项实际 UID/GID |
| backup-complete | 全部快照已验证可用于恢复 |
| intent | 在有副作用操作之前持久记录意图 |
| current | 唯一受控绝对 selector，指声明的物理版本 Root |
| allow-data-loss | 明确接受丢弃备份之后可能发生的业务写入 |
| ready | 实际程序/PID/UID/hash与正确产品业务检查同时通过 |

应能解释为何 stop 后还要排他锁，为什么恢复程序必须同时恢复配置/数据，为什么启动超时不能默认丢弃写入，以及为何软件版增长不要求数据标签增长。
