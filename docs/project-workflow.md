# Sarmg Upgrade 工作流程与流程树

## 1. 当前流程树

```text
sarmg-upgrade 0.2.0
├─ 能力发现
│  ├─ support --json
│  ├─ catalog --json
│  └─ inspect-manifest
├─ Media Backup 0.2.0 当前组合状态
│  ├─ backup-media
│  ├─ verify-media-backup
│  ├─ restore-media
│  └─ recover-media-restore
├─ 当前 SQLite-only 状态
│  ├─ Host Monitoring 0.7.0：backup / verify / restore / recover
│  └─ Sunshine Manager 0.7.0：keyed backup / verify / restore
├─ 暂未实现
│  ├─ Sentinel 当前组合备份/恢复
│  ├─ Dufs 当前组合备份/恢复
│  └─ 所有历史升级 edge
└─ 交付
   ├─ tests + clippy + supply-chain checks
   └─ source-bound archive + support snapshot + SBOM + provenance
```

## 2. 能力发现先于命令执行

```text
验证 binary/release checksum
 -> sarmg-upgrade support --json
 -> 确认 product + operation + exact current version
 -> sarmg-upgrade catalog --json
 -> 确认资源是 SQLite-only 还是 composite
 -> 不在 support 中：立即停止，不用相近命令替代
```

`catalog` 表示产品拥有哪些持久资源，不表示工具已经实现该产品的完整备份。唯一机器事实是当前 binary
输出的 `support`；当前每个 `upgrade_edges` 数组都必须为空。

## 3. 当前备份流程

```text
校验显式 product/路径/key
 -> 取得产品约定的 shared maintenance lock
 -> 验证 code-owned 当前 Schema identity
 -> Sunshine：实际认证全部密文
 -> 创建私有 pending output（目标必须不存在）
 -> SQLite online snapshot；组合产品再复制 data tree
 -> 核对文件 type/mode/size/SHA/tree inventory
 -> 再验证 copied state
 -> 最后写 manifest 并 fsync
 -> no-replace rename 发布 output
 -> 重新执行对应 verify
```

Media 的 database 与 data tree 是一个组合代，不能拆成两个命令。Host/Sunshine 才能使用 SQLite-only 路径。

## 4. 当前恢复流程

```text
严格解析 manifest
 -> 验证每个资源、Schema、Hash 和 external requirement
 -> 取得 exclusive maintenance lock
 -> 核对目标版本和 replace policy
 -> 在目标同一文件系统创建 incoming stage
 -> 再验证 incoming
 -> 创建相邻私有 recovery + durable journal
 -> 保存原 database/sidecars（如允许 replace）
 -> 原子安装 incoming
 -> 验证已安装当前代
 -> fsync 并清理 recovery
```

恢复不自动停止或启动产品。操作者必须先阻止 systemd、launchd、Windows Service、watchdog 或手工进程
重启目标服务。工具的锁是最后一道并发保护，不是服务管理器。

## 5. 中断恢复流程

```text
命令报告 recovery path
 -> 保持产品停止
 -> 不编辑/移动/删除 recovery
 -> 修复空间、只读挂载等环境问题
 -> 使用相同 binary/product/version/path
 -> action=commit：证明 incoming 后完成安装
 -> action=rollback：证明 preserved original 后恢复
 -> verify + 产品 doctor + smoke
```

Host 的 SQLite restore 可用 `recover-sqlite`。Media 使用 `recover-media-restore`。Sunshine 的 keyed
SQLite restore 当前不对外声明 recover；若出现无法自动清理的 recovery，停止并保全证据，不要用 Host
命令绕过密文认证。

## 6. 为什么当前没有升级流程

开发期数据格式不是长期合同。试验性历史 SQL/adapter 会迫使产品和工具维护尚未承诺的旧语义，因此已
删除。当前流程树不存在 `upgrade-sqlite`、`upgrade-sentinel`、`upgrade-dufs`、source-backup 或
upgrade-recovery 分支。旧开发数据应重新部署；确有保留价值时由一次性、独立审核的数据处理仓库完成，
不得把临时代码并回产品运行时。

未来首个 edge 的准入流程为：稳定 source/target 身份 -> 独立 fixture 和恶意负例 -> immutable source
backup -> 从零构建 target -> external key -> 停机锁 -> durable journal -> 全故障点 commit/rollback ->
support/release/docs。任何一步未完成，都不得出现在支持矩阵。

## 7. 正式发行流程

```text
clean checkout + annotated exact tag
 -> fmt/check/clippy/test
 -> workflow supply-chain validation
 -> build source-bound binary
 -> capture support/catalog
 -> generate SBOM/environment/provenance
 -> immutable build artifact
 -> publish job 不 checkout source
 -> sign SHA256SUMS
 -> 解包复验 binary/support/checksum
 -> 发布固定 archive；拒绝覆盖既有 asset
```

发布后必须抽查 `support --json`：若出现未经审核的历史 edge，发行失败。
