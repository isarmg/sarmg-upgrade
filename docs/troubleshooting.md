# 排查升级和恢复问题

先保存失败输出和本次恢复目录，再检查事务。命令使用计划中的真实路径，以具有本次维护权限的身份执行：

```sh
xssc inspect-upgrade --work-directory /absolute/private/recovery
```

输出给出持久阶段、程序身份、维护门和状态校验结果。无法读取或验证表示需要调查；保留日志、快照与维护门，让恢复流程据此判断下一步。

| 现象或错误 | 检查 | 下一步 |
|---|---|---|
| `SERVER_MUST_BE_STOPPED` | 主服务和全部辅助 writer 的 `LoadState`、`MainPID`、`ActiveState` | 协调停止写入方及自动重启管理器；预期 loaded、PID 0、inactive/failed |
| `CURRENT_RELEASE_INCOMPATIBLE` | 当前程序的完整 `release-identity` 与签名 `source_identity` | 取得匹配当前软件身份的制品定义 |
| `STATE_INCOMPATIBLE` 或状态校验失败 | 当前配置、结构身份与源/目标普通只读校验输出 | 处理明确的数据或配置问题；当前流程要求相同源/目标结构 |
| 签名或公钥身份错误 | 独立公钥 DER 指纹、签名文件、清单和目标文件是否来自同一发行 | 从可信发行来源重新核对文件和身份 |
| 空间不足 | 剩余空间、计划预算、原完整发行目录及持久资源总量 | 释放与本次事务无关的空间；保留事务和快照 |
| 启动或就绪超时 | `systemctl status`、产品日志、实际 PID/exe 和 `/readyz` | 修正服务环境后按事务阶段决定恢复；启动超时仍可能已有新写入 |
| `RECOVERY_AUTHORIZATION_REQUIRED` | 是否已到 `start-intent` 或 `recovery-start-intent` | 默认保留新数据；仅在明确接受损失后使用数据损失授权 |
| 未知程序、发行目录 inode 或坏备份 | 实际对象是否被其他操作替换、完整快照能否验证 | 保全现状与输出，调查来源后再恢复 |

## 查看服务日志

将 `xocs.service` 换成计划中的实际 unit：

```sh
systemctl status xocs.service --no-pager
journalctl -u xocs.service -n 100 --no-pager
```

成功升级阶段为 `ready`；成功恢复为 `rolled-back`，两者都包含实际业务就绪验证。恢复命令和丢失范围说明见[失败恢复](offline-upgrades.md#失败恢复与数据损失授权)，阶段含义见[事务参考](reference/transaction-stages.md)。
