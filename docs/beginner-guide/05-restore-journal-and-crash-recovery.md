# 05. 恢复、Journal 与中断处置

## 5.1 恢复前验证

先严格验证 backup、全部资源、code identity 和 external key，再取得排他锁并验证目标策略。`--replace-
existing` 是明确授权，不表示可跳过 preserved original 或路径检查。

## 5.2 Stage

在目标相邻同文件系统建立私有 stage，复制/生成来件，设置最终 mode/owner，重新计算 Hash/Schema/tree 并
同步。目标在 stage 完整前不变。

## 5.3 Recovery journal

journal 记录 operation identity、产品/版本、路径身份、original/incoming 名称、阶段和预期 Hash。它先于
第一次目标 mutation 持久化，并在每阶段更新后同步目录。

## 5.4 安装阶段

```text
prepared -> original preserved -> incoming installed -> installed verified -> committed
```

组合状态可有数据库、树、config 等多个子阶段。任意崩溃后不能仅从“目标存在”推断完成。

## 5.5 Commit

重新验证 incoming/installed 是精确 target，必要时认证密文，确认所有组件一致，完成目录同步，再删除
preserved original 和 journal。无法证明目标正确时 commit 必须拒绝。

## 5.6 Rollback

验证 preserved original 与 journal identity，把已安装来件移出/保全，再原子恢复原件并同步。rollback
恢复原始字节，不把旧状态解释为当前产品可运行；之后只能用匹配产品或重新升级。

## 5.7 操作者中断流程

保持所有服务停止；保存错误和 binary SHA；不移动/编辑 recovery；修复空间/挂载/key 等环境问题；用
完全相同产品、版本、路径和身份运行对应 `recover-* --action commit|rollback`。

## 5.8 禁止手工拼接

不要把 stage 文件复制到目标、删除 journal、重命名 preserved 目录或编辑阶段值。手工动作会破坏工具
用于证明的身份，使后续安全恢复不可判定。

## 5.9 测试点

在每个 rename/fsync 前后故障注入；验证重复 recover 幂等、错误 action/路径/key 拒绝、journal 篡改拒绝、
跨设备拒绝和最终无临时残留。
