# 07. 为什么当前没有历史升级 Edge

## 7.1 先区分“未来设计”与“当前功能”

历史 edge 是一个精确 source version 到精确 target version 的有向转换。`sarmg-upgrade 0.2.0` 当前没有
任何 edge：`support --json` 的数组为空，CLI 不注册 `upgrade-*`，源码也没有旧 Schema SQL/parser。
本章后半解释未来准入标准，不表示命令已经存在。

## 7.2 为什么开发期删除 edge

开发期 Schema、密文、目录和业务不变量仍可能快速变化。为每个试验版本保留 adapter，会让团队误以为
这些格式已经承诺长期迁移，并迫使测试覆盖无实际用户价值的旧分支。更严重的是，旧 parser 和宽松转换
会成为长期攻击面。因此开发数据默认重新部署；需要一次性保留的数据由独立、短生命周期且单独审核的
处理仓库完成。

## 7.3 如何证明“确实没有”

```bash
sarmg-upgrade support --json
sarmg-upgrade --help
rg 'upgrade-|from-version|to-version' src
```

第一条是机器事实；第二条验证没有命令入口；第三条是开发审查辅助。文档、README 或 catalog 中出现产品
名称，都不能替代 support。

## 7.4 当前数据库为何拒绝 migration ledger

当前产品和本工具只接受 code-owned 当前 Schema identity。数据库中出现 `_sqlx_migrations` 会失败关闭，
而不是自动回放或猜测迁移。这可防止旧开发数据库通过“表看起来相似”混入当前备份。

## 7.5 未来 edge 的 source 证明

若首个稳定版本之后确需升级，adapter 必须验证 metadata、规范 Schema SHA、精确 migration ledger/checksum、
资源合同、密文和业务不变量。任何字段不符就拒绝，不能选择“最接近”的 adapter。

## 7.6 Source backup first

未来 adapter 必须先从受停机锁保护的 raw clone 建立 immutable source backup。只有 source backup 完整发布
并可独立 verify，才可从 target current SQL/code 从零构建新代；禁止在 source 原件上逐步 `ALTER TABLE`。

## 7.7 密文和组合资源

external key 只能在运行时提供，raw bytes 不进入包。转换必须先认证 source，再按 target 唯一算法/AAD
生成并全量验证。Sentinel、Dufs 等组合产品还必须把 config、companion、recordings/shared tree、owner、
mode、link 和容量预算纳入同一 journal，不能只升级 SQLite。

## 7.8 Target 与切换证明

target 必须通过和目标产品相同的 code-owned metadata/Schema/业务不变量、全部密文和资源 inventory 检查。
随后才可创建 durable recovery journal，保存原代，并在同文件系统原子安装。每个持久边界都要有 kill/
error 故障注入以及 commit/rollback 测试。

## 7.9 遇到旧开发数据怎么办

停止操作并保留只读副本。不要手改 metadata/SHA，不要复制旧 SQL 回本仓，不要把 generic backup 当迁移。
优先重新部署并从业务源重新导入。若确有不可重建价值，建立独立一次性处理仓库，明确 source/target、
fixture、审核者和销毁时间；产品 runtime 和当前工具仍不增加兼容分支。
