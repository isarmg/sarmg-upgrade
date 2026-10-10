# 5. 事务日志和中断恢复

事务日志记录操作 UUID、计划、签名目标、原程序摘要/权限模式、完整快照和目录身份。每步操作意图先持久再执行；门与父目录同步。

| 已到阶段 | 恢复判断 |
|---|---|
| `prechecked` | 已由操作者停服，尚未改写持久状态；可关闭未修改记录 |
| `backup-started` | 备份未完成，不使用部分快照 |
| `backup-complete` / `validated` | 全组备份可验证，尚未交接运行权 |
| `switch-intent` / `switched` | 检查实际程序/发行根目录，而非只信阶段字段 |
| `start-intent` / `ready` | 可能已有新写入，默认拒绝丢弃 |
| `recovery-restoring` | 门+排他锁下续接整组恢复 |
| `recovery-start-intent` | 原程序也可能产生恢复后新写入 |
| `rolled-back` | 恢复已验证，重复调用只报告结果 |

```sh
xssc inspect-upgrade --work-directory /absolute/private/recovery
xssc recover-upgrade --work-directory /absolute/private/recovery
```

明确接受丢弃备份之后的业务写入，才添加 `--allow-data-loss`。启动失败、程序退出和端口不可达均不能证明没有写入。

完整源发行根目录若仍是原 inode但资产损坏，先持久修复意图，将损坏树移动留证，再重建原树并切回 `current`。未知替换 inode/对象或损坏备份拒绝覆盖。恢复最后要求实际原程序与正确业务就绪。
