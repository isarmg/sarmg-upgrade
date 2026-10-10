# 升级流程与实现导航

工具只有一条写入流程：从当前支持的完整状态升级到受签名的未来发行物。源/目标状态合同必须完全相同。结构变化要等待实际未来发行提供对应实现，当前没有历史数据 parser 或转换命令。

## 执行流程

```text
读取私有 plan
  → 固定信任锚、验签、ELF/平台/hash、发行 identity
  → 封存完整执行闭包、校验工具受签名 artifact 与实际发行身份
  → 核对 systemd User/Group 与数据属主
  → 确认主服务和全部额外 writer 已由操作者停止
  → 持久 prechecked / maintenance-intent、设置维护门
  → 再查 MainPID=0，取得排他维护锁并复查
  → 整组 program/config/state/completeRoot 快照
  → inventory/hash/UID/GID 验证、backup-complete
  → 当前源与封存目标只读校验、核对 state_paths 覆盖
  → 原子程序或完整 Root/current 切换
  → 持久 start-intent / 可能写入标记
  → 清门、显式释放锁、启动
  → MainPID/UID/exeSHA + 业务 HTTP ready
  → ready
```

只有完整快照通过验证后才能标记 `backup-complete`。只读校验前后整组 inventory 和属主必须相同。完整 Root 的所有资产及本工具的签名制品定义参与验证，不能仅替换其中的 ELF。

## 恢复流程

```text
读取原 journal、核对路径和目录身份
  → 检查新程序是否可能已有写入
  → 必要时要求 --allow-data-loss
  → 确认操作者已停止全部 writer
  → 持久 recovery-maintenance-intent、设置门、取得排他锁并复查
  → 验证全部原始备份与当前可识别程序
  → 恢复原配置/数据/UID/GID/program/completeRoot
  → 当前原程序只读验证
  → 持久 recovery-start-intent / 可能写入标记
  → 清门、显式释放锁、启动原程序
  → 实际原程序与业务 ready
  → rolled-back
```

未完成备份不可恢复。未知程序、未知 Root inode、损坏快照停止操作。恢复启动后发生中断，再次恢复也须保护新增写入。已完成 `rolled-back` 再次调用只报告结果。

## 代码归属

| 实现 | 责任 |
|---|---|
| `src/main.rs` | apply / inspect / recover / support / catalog CLI |
| `src/upgrade/mod.rs` | 严格 DTO、信任验证、systemd、阶段与恢复 |
| `src/upgrade/snapshot.rs` | nofollow、bounded inventory、复制、逐项 UID/GID、完整恢复 |
| `src/upgrade/native_release.rs` | 产品完整 Root、树摘要、current、NOREPLACE 发布、原树修复 |
| `src/upgrade/process.rs` | stdout/stderr 双管道预算、超时回收、固定文件描述符和服务身份执行 |
| `src/upgrade/execution.rs` | 行政属主与 systemd 身份、root 控制执行目录 |
| `scripts/stage-upgrade-release.py` | 统一产品制品打包和 Ed25519 签名 |
| `src/upgrade/tests.rs` | 真实机制/故障/权限/中断测试 |
| `src/upgrade/product_tests.rs` | 真实当前产品 init、校验、run、ready 和恢复 |

xcsc 内部模块提供中立合同、锁、安全文件和日志；产品普通诊断负责业务校验。工具拥有签名、备份、切换与恢复流程，不注入 shell hooks。

## CLI 输出

`apply-upgrade` 和 `recover-upgrade` 成功输出阶段与 journal。失败返回稳定错误码及恢复目录，不反射子进程 stderr 或内部 error chain。`inspect-upgrade` 检查完成备份、现行程序与原始快照关系、门和 Root，不把无法读取解释为安全未修改。结构化日志通过 xcsc::log记录 `xssc.phase_changed` 和操作 UUID。

完整字段与操作者步骤见[离线升级与恢复](offline-upgrades.md)。
